//! Local control API (JSON over HTTP on 127.0.0.1), used by the desktop UI.
//! Every request needs `Authorization: Bearer <token>`; the token file is only
//! readable by root and the admin group.

use crate::state::{now, top, App};
use crate::{dns, upstream};
use axum::extract::{Query, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use portward_core::model::{AppSummary, Connection, Stats, Status};
use portward_core::{normalize_domain, Config, RuleSet};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

type S = State<Arc<App>>;

struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}"))
    }
}
fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

async fn auth(State(app): S, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| constant_eq(t.as_bytes(), app.token.as_bytes()))
        .unwrap_or(false);
    if !ok {
        return ApiError(StatusCode::UNAUTHORIZED, "missing or wrong token".into()).into_response();
    }
    next.run(req).await
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/connections", get(connections))
        .route("/api/v1/apps", get(apps))
        .route("/api/v1/apps/domains", get(app_domains))
        .route("/api/v1/dns", get(dns_events))
        .route("/api/v1/stats", get(stats))
        .route("/api/v1/config", get(get_config).put(put_config))
        .route("/api/v1/rules/quick", post(quick_rule))
        .route("/api/v1/check", get(check))
        .route("/api/v1/filterlists", get(filterlists))
        .route("/api/v1/filterlists/update", post(update_lists))
        .route("/api/v1/cache/flush", post(flush))
        .route("/api/v1/pause", post(pause))
        .route("/api/v1/history/connections", get(history))
        .route("/api/v1/exposure", get(exposure))
        .route("/api/v1/exposure/recheck", post(recheck))
        .route("/api/v1/traffic", get(traffic))
        .route("/api/v1/packets", get(packets))
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        .with_state(app)
}

async fn status(State(app): S) -> Json<Status> {
    let cfg = app.cfg.read();
    let mut warnings = app.warnings.read().clone();
    warnings.extend(app.rule_errors.read().iter().map(|e| format!("Regra inválida: {e}")));
    let filter = cfg.mode == portward_core::config::Mode::Filter;
    if filter && !*app.system_dns_managed.read() {
        warnings.push(format!(
            "O DNS do sistema não aponta para o Portward. Só apps que consultam {} diretamente são filtrados.",
            cfg.dns_listen
        ));
    }
    Json(Status {
        mode: cfg.mode,
        capture_interfaces: app.capture_ifaces.read().clone(),
        version: env!("CARGO_PKG_VERSION").into(),
        build: option_env!("PORTWARD_BUILD").unwrap_or("dev").into(),
        started: app.started,
        running_as_root: app.root,
        dns_listen: cfg.dns_listen.clone(),
        dns_ok: *app.dns_ok.read(),
        system_dns_managed: *app.system_dns_managed.read(),
        pf_active: *app.pf_active.read(),
        paused: cfg.paused,
        blocked_domains: app.filters.read().len(),
        last_upstream: app.last_upstream.read().clone(),
        counters: app.counters.lock().clone(),
        warnings,
    })
}

#[derive(Deserialize)]
struct ConnQ {
    #[serde(default)]
    active: bool,
    #[serde(default)]
    app: String,
}

async fn connections(State(app): S, Query(q): Query<ConnQ>) -> Json<Vec<Connection>> {
    let conns = app.conns.lock();
    let mut v: Vec<Connection> = conns
        .values()
        .filter(|c| !q.active || c.active)
        .filter(|c| q.app.is_empty() || c.process.app_path == q.app)
        .cloned()
        .collect();
    v.sort_by(|a, b| b.first_seen.cmp(&a.first_seen));
    Json(v)
}

async fn apps(State(app): S) -> Result<Json<Vec<AppSummary>>, ApiError> {
    let days = app.cfg.read().history_days;
    let since = now() - days as i64 * 86400;
    let mut map: HashMap<String, AppSummary> = HashMap::new();
    let store = &app.store;
    let totals = tokio::task::block_in_place(|| store.app_totals(since))?;
    for (path, name, total, blocked, last) in totals {
        map.insert(
            path.clone(),
            AppSummary { path, name, active_connections: 0, total_connections: total as usize, blocked_connections: blocked as usize, domains: vec![], last_seen: last },
        );
    }
    let mut domains: HashMap<String, BTreeSet<String>> = HashMap::new();
    for c in app.conns.lock().values() {
        let e = map.entry(c.process.app_path.clone()).or_insert_with(|| AppSummary {
            path: c.process.app_path.clone(),
            name: c.process.name.clone(),
            active_connections: 0,
            total_connections: 0,
            blocked_connections: 0,
            domains: vec![],
            last_seen: c.last_seen,
        });
        if c.active {
            e.active_connections += 1;
        }
        e.last_seen = e.last_seen.max(c.last_seen);
        if let Some(d) = &c.domain {
            domains.entry(c.process.app_path.clone()).or_default().insert(d.clone());
        }
    }
    let mut v: Vec<AppSummary> = map
        .into_values()
        .map(|mut a| {
            a.domains = domains.remove(&a.path).map(|s| s.into_iter().take(20).collect()).unwrap_or_default();
            a
        })
        .collect();
    v.sort_by(|a, b| b.active_connections.cmp(&a.active_connections).then(b.last_seen.cmp(&a.last_seen)));
    Ok(Json(v))
}

#[derive(Deserialize)]
struct PathQ {
    path: String,
}

async fn app_domains(State(app): S, Query(q): Query<PathQ>) -> Result<Json<Value>, ApiError> {
    let since = now() - app.cfg.read().history_days as i64 * 86400;
    let store = &app.store;
    let rows = tokio::task::block_in_place(|| store.app_domains(&q.path, since))?;
    Ok(Json(json!(rows.into_iter().map(|(d, n)| json!({"key": d, "count": n})).collect::<Vec<_>>())))
}

#[derive(Deserialize)]
struct DnsQ {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    blocked: bool,
    #[serde(default)]
    q: String,
}
fn default_limit() -> usize {
    300
}

async fn dns_events(State(app): S, Query(q): Query<DnsQ>) -> Json<Value> {
    let needle = q.q.to_ascii_lowercase();
    let ev = app.dns_events.lock();
    let v: Vec<_> = ev
        .iter()
        .rev()
        .filter(|e| !q.blocked || e.blocked)
        .filter(|e| needle.is_empty() || e.domain.contains(&needle))
        .take(q.limit.min(5000))
        .cloned()
        .collect();
    Json(json!(v))
}

async fn stats(State(app): S) -> Json<Stats> {
    let since = now() - 3600;
    let per_minute = app.minutes.lock().iter().filter(|m| m.minute >= since).cloned().collect();
    let mut apps: HashMap<String, u64> = HashMap::new();
    let mut countries: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut orgs: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut servers = BTreeSet::new();
    let (mut bin, mut bout) = (0, 0);
    for c in app.conns.lock().values() {
        *apps.entry(c.process.name.clone()).or_default() += 1;
        bin += c.bytes_in;
        bout += c.bytes_out;
        if c.scope == "internet" {
            servers.insert(c.remote_ip.clone());
            if let Some(g) = &c.geo {
                let cname = match (&g.country, &g.country_name) {
                    (Some(cc), Some(n)) => format!("{cc}|{n}"),
                    (Some(cc), None) => cc.clone(),
                    _ => "?".into(),
                };
                countries.entry(cname).or_default().insert(c.remote_ip.clone());
                if let Some(o) = &g.org {
                    orgs.entry(o.clone()).or_default().insert(c.remote_ip.clone());
                }
            }
        }
    }
    let count = |m: HashMap<String, BTreeSet<String>>| -> HashMap<String, u64> { m.into_iter().map(|(k, v)| (k, v.len() as u64)).collect() };
    Json(Stats {
        per_minute,
        top_blocked: top(&app.top_blocked.lock(), 15),
        top_domains: top(&app.top_domains.lock(), 15),
        top_apps: top(&apps, 10),
        countries: top(&count(countries), 12),
        orgs: top(&count(orgs), 12),
        bytes_in: bin,
        bytes_out: bout,
        remote_servers: servers.len(),
    })
}

async fn get_config(State(app): S) -> Json<Value> {
    Json(json!({ "config": *app.cfg.read(), "rule_errors": *app.rule_errors.read() }))
}

async fn put_config(State(app): S, Json(new): Json<Config>) -> Result<Json<Value>, ApiError> {
    let (_, errs) = RuleSet::parse(&new.rules);
    if !errs.is_empty() {
        return Err(bad(errs.join("; ")));
    }
    if new.upstreams.is_empty() {
        return Err(bad("é preciso pelo menos um servidor DNS"));
    }
    let old = app.cfg.read().clone();
    *app.cfg.write() = new.clone();
    app.save_config()?;
    if old.mode != new.mode {
        // Switching modes means starting/stopping the DNS server, the system DNS
        // takeover and pf. Restart cleanly; launchd brings us back up.
        let a = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            a.shutdown.notify_one();
        });
        return Ok(Json(json!({ "ok": true, "restarting": true })));
    }
    if old.upstreams != new.upstreams {
        *app.http_doh.write() = upstream::build_client(&new.upstreams);
    }
    if old.filter_lists != new.filter_lists || old.filter_subdomains != new.filter_subdomains {
        app.lists_changed.notify_one();
    }
    app.reload_rules();
    Ok(Json(json!({ "ok": true, "restart_required": old.dns_listen != new.dns_listen || old.api_listen != new.api_listen })))
}

#[derive(Deserialize)]
struct QuickRule {
    /// `block` or `permit`
    action: String,
    entity: String,
}

async fn quick_rule(State(app): S, Json(r): Json<QuickRule>) -> Result<Json<Value>, ApiError> {
    let sign = match r.action.as_str() {
        "block" => "-",
        "permit" => "+",
        _ => return Err(bad("ação deve ser block ou permit")),
    };
    let entity = r.entity.trim();
    let line = format!("{sign} {entity}");
    portward_core::rules::parse_rule(&line).map_err(bad)?;
    {
        let mut cfg = app.cfg.write();
        cfg.rules.retain(|l| l.trim().get(1..).map(str::trim) != Some(entity));
        cfg.rules.insert(0, line.clone());
    }
    app.save_config()?;
    app.reload_rules();
    crate::sysdns_flush(&app);
    Ok(Json(json!({ "ok": true, "rule": line })))
}

#[derive(Deserialize)]
struct CheckQ {
    domain: String,
}

async fn check(State(app): S, Query(q): Query<CheckQ>) -> Json<Value> {
    let d = normalize_domain(&q.domain);
    let dec = dns::decide(&app, &d);
    Json(json!({ "domain": d, "decision": dec }))
}

async fn filterlists(State(app): S) -> Json<Value> {
    Json(json!(*app.list_status.read()))
}

async fn update_lists(State(app): S) -> Json<Value> {
    *app.force_list_update.lock() = true;
    app.lists_changed.notify_one();
    Json(json!({ "ok": true }))
}

async fn flush(State(app): S) -> Json<Value> {
    app.flush_cache();
    crate::sysdns_flush(&app);
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct PauseQ {
    paused: bool,
}

async fn pause(State(app): S, Json(p): Json<PauseQ>) -> Result<Json<Value>, ApiError> {
    app.cfg.write().paused = p.paused;
    app.save_config()?;
    app.reload_rules();
    crate::sysdns_flush(&app);
    Ok(Json(json!({ "ok": true, "paused": p.paused })))
}

#[derive(Deserialize)]
struct HistQ {
    #[serde(default)]
    q: String,
    #[serde(default)]
    app: String,
    #[serde(default = "default_hours")]
    hours: i64,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_hours() -> i64 {
    24
}

async fn history(State(app): S, Query(q): Query<HistQ>) -> Result<Json<Value>, ApiError> {
    let store = &app.store;
    let rows = tokio::task::block_in_place(|| {
        store.connections(&q.q, &q.app, now() - q.hours * 3600, q.limit.min(5000) as u32)
    })?;
    Ok(Json(json!(rows)))
}

#[derive(serde::Serialize, Default)]
struct Server {
    host: String,
    ips: BTreeSet<String>,
    country: Option<String>,
    country_name: Option<String>,
    org: Option<String>,
    /// app path → app name
    apps: std::collections::BTreeMap<String, String>,
    exposed_ips: BTreeSet<String>,
    interfaces: BTreeSet<String>,
    via_vpn: bool,
    connections: usize,
    active: usize,
    bytes_in: u64,
    bytes_out: u64,
    last_seen: i64,
}

/// "Who sees my IP": your public identities per interface, and every internet
/// server that saw one of them, grouped by host.
async fn exposure(State(app): S) -> Json<Value> {
    let mut map: HashMap<String, Server> = HashMap::new();
    for c in app.conns.lock().values().filter(|c| c.scope == "internet") {
        let host = c.domain.clone().unwrap_or_else(|| c.remote_ip.clone());
        let s = map.entry(host.clone()).or_insert_with(|| Server { host, ..Default::default() });
        s.ips.insert(c.remote_ip.clone());
        if let Some(g) = &c.geo {
            s.country = s.country.take().or(g.country.clone());
            s.country_name = s.country_name.take().or(g.country_name.clone());
            s.org = s.org.take().or(g.org.clone());
        }
        s.apps.insert(c.process.app_path.clone(), c.process.name.clone());
        if let Some(e) = &c.exposed_ip {
            s.exposed_ips.insert(e.clone());
        }
        if let Some(i) = &c.interface {
            s.interfaces.insert(i.clone());
        }
        s.via_vpn |= c.via_vpn;
        s.connections += 1;
        if c.active {
            s.active += 1;
        }
        s.bytes_in += c.bytes_in;
        s.bytes_out += c.bytes_out;
        s.last_seen = s.last_seen.max(c.last_seen);
    }
    let mut servers: Vec<Server> = map.into_values().collect();
    servers.sort_by(|a, b| b.active.cmp(&a.active).then(b.last_seen.cmp(&a.last_seen)));
    let mut ifaces = app.interfaces.read().clone();
    for i in &mut ifaces {
        if i.geo_v4.is_none() {
            i.geo_v4 = i.public_ipv4.as_ref().and_then(|ip| ip.parse().ok()).and_then(|ip| app.geo.lookup(ip));
        }
        if i.geo_v6.is_none() {
            i.geo_v6 = i.public_ipv6.as_ref().and_then(|ip| ip.parse().ok()).and_then(|ip| app.geo.lookup(ip));
        }
    }
    Json(json!({ "interfaces": ifaces, "servers": servers, "geo_loaded": app.geo.loaded() }))
}

async fn recheck(State(app): S) -> Json<Value> {
    app.recheck_ip.notify_one();
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct TrafficQ {
    #[serde(default)]
    app: String,
}

/// Last hour of traffic in 10 s buckets, for one app or (app = "") everything.
async fn traffic(State(app): S, Query(q): Query<TrafficQ>) -> Json<Value> {
    let tr = app.traffic.lock();
    Json(json!(tr.get(&q.app).map(|v| v.iter().cloned().collect::<Vec<_>>()).unwrap_or_default()))
}

#[derive(Deserialize)]
struct PacketQ {
    /// Comma separated remote IPs to filter on (a server can have several).
    #[serde(default)]
    ips: String,
    /// Only packets newer than this id (for live tailing).
    #[serde(default)]
    after: u64,
    /// Only packets of connections owned by this app (app path).
    #[serde(default)]
    app: String,
    #[serde(default = "default_pkt_limit")]
    limit: usize,
}
fn default_pkt_limit() -> usize {
    400
}

/// Recent captured packets (newest last), each tagged with the app that owns
/// its connection when known.
async fn packets(State(app): S, Query(q): Query<PacketQ>) -> Json<Value> {
    let ips: BTreeSet<String> = q.ips.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let owner: HashMap<String, (String, String)> = app
        .conns
        .lock()
        .values()
        .map(|c| (format!("{}|{}|{}", c.protocol, c.local, c.remote), (c.process.name.clone(), c.process.app_path.clone())))
        .collect();
    let ring = app.packets.lock();
    let mut out: Vec<Value> = ring
        .iter()
        .rev()
        .filter(|p| p.id > q.after)
        .filter(|p| q.app.is_empty() || owner.get(&format!("{}|{}|{}", p.proto, p.local, p.remote)).map(|(_, path)| path == &q.app).unwrap_or(false))
        .filter(|p| {
            if ips.is_empty() {
                return true;
            }
            let ip = p.remote.rsplit_once(':').map(|(h, _)| h.trim_start_matches('[').trim_end_matches(']')).unwrap_or("");
            ips.contains(ip)
        })
        .take(q.limit.min(5000))
        .map(|p| {
            let mut v = json!(p);
            if let Some((name, path)) = owner.get(&format!("{}|{}|{}", p.proto, p.local, p.remote)) {
                v["app"] = json!(name);
                v["app_path"] = json!(path);
            }
            v
        })
        .collect();
    out.reverse();
    Json(json!(out))
}
