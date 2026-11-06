//! The filtering DNS server.
//!
//! macOS sends every lookup to the servers in the network settings; once those
//! point at us we see (and can block) every domain any app resolves.

use crate::procinfo::{self, Proto};
use crate::state::{now, App, CacheEntry};
use crate::upstream;
use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use portward_core::config::BlockResponse;
use portward_core::model::{DnsEvent, ProcessInfo};
use portward_core::{normalize_domain, Decision};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tracing::{debug, error, info, warn};

const BLOCK_TTL: u32 = 60;

pub async fn serve(app: Arc<App>, listen: SocketAddr) -> anyhow::Result<()> {
    let udp = Arc::new(UdpSocket::bind(listen).await?);
    let tcp = TcpListener::bind(listen).await?;
    info!("DNS server listening on {listen} (udp+tcp)");
    *app.dns_ok.write() = true;

    let app_t = app.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, peer)) = tcp.accept().await else { continue };
            let app = app_t.clone();
            tokio::spawn(async move {
                loop {
                    let mut len = [0u8; 2];
                    if s.read_exact(&mut len).await.is_err() {
                        break;
                    }
                    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
                    if s.read_exact(&mut buf).await.is_err() {
                        break;
                    }
                    let out = handle(&app, &buf, peer, Proto::Tcp, usize::MAX).await;
                    if s.write_all(&(out.len() as u16).to_be_bytes()).await.is_err()
                        || s.write_all(&out).await.is_err()
                    {
                        break;
                    }
                }
            });
        }
    });

    let mut buf = vec![0u8; 4096];
    loop {
        let (n, peer) = match udp.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(e) => {
                error!("udp recv: {e}");
                continue;
            }
        };
        let pkt = buf[..n].to_vec();
        let (app, udp) = (app.clone(), udp.clone());
        tokio::spawn(async move {
            let out = handle(&app, &pkt, peer, Proto::Udp, 0).await;
            let _ = udp.send_to(&out, peer).await;
        });
    }
}

fn qtype_name(t: RecordType) -> String {
    format!("{t}")
}

/// Who sent the query? Uses the monitor's port map first, then a full scan.
async fn attribute(app: &App, peer: SocketAddr, proto: Proto) -> Option<ProcessInfo> {
    if !peer.ip().is_loopback() {
        return None;
    }
    let port = peer.port();
    let known = app.port_owner.read().get(&port).copied();
    let pid = match known {
        Some(p) => Some(p),
        None => tokio::task::spawn_blocking(move || procinfo::find_pid_by_local_port(proto, port))
            .await
            .ok()
            .flatten(),
    }?;
    Some(procinfo::process_info(pid))
}

pub fn decide(app: &App, domain: &str) -> Option<Decision> {
    let cfg = app.cfg.read();
    if cfg.paused {
        return None;
    }
    let subs = cfg.filter_subdomains;
    drop(cfg);
    if let Some(d) = app.rules.read().check_domain(domain) {
        return Some(d);
    }
    let filters = app.filters.read().clone();
    filters
        .lookup(domain, subs)
        .map(|list| Decision::block(format!("lista {list}")))
}

fn blocked_response(req: &Message, mode: BlockResponse) -> Message {
    let mut resp = Message::response(req.metadata.id, req.metadata.op_code);
    resp.metadata.recursion_desired = req.metadata.recursion_desired;
    resp.metadata.recursion_available = true;
    resp.queries = req.queries.clone();
    let q = &req.queries[0];
    match (mode, q.query_type()) {
        (BlockResponse::ZeroIp, RecordType::A) => {
            resp.add_answer(Record::from_rdata(q.name().clone(), BLOCK_TTL, RData::A(A(Ipv4Addr::UNSPECIFIED))));
        }
        (BlockResponse::ZeroIp, RecordType::AAAA) => {
            resp.add_answer(Record::from_rdata(q.name().clone(), BLOCK_TTL, RData::AAAA(AAAA(Ipv6Addr::UNSPECIFIED))));
        }
        (BlockResponse::ZeroIp, _) => {}
        (BlockResponse::Nxdomain, _) => resp.metadata.response_code = ResponseCode::NXDomain,
    }
    if let Some(e) = &req.edns {
        resp.edns = Some(e.clone());
    }
    resp
}

fn encode(mut msg: Message, max_udp: usize) -> Vec<u8> {
    let bytes = msg.to_vec().unwrap_or_default();
    if bytes.len() > max_udp {
        msg = msg.truncate();
        return msg.to_vec().unwrap_or_default();
    }
    bytes
}

fn answers_of(msg: &Message) -> Vec<String> {
    msg.answers
        .iter()
        .filter_map(|r| match &r.data {
            RData::A(a) => Some(a.0.to_string()),
            RData::AAAA(a) => Some(a.0.to_string()),
            RData::CNAME(c) => Some(format!("CNAME {}", normalize_domain(&c.0.to_ascii()))),
            _ => None,
        })
        .collect()
}

async fn handle(app: &Arc<App>, pkt: &[u8], peer: SocketAddr, proto: Proto, max_udp: usize) -> Vec<u8> {
    let start = Instant::now();
    let req = match Message::from_vec(pkt) {
        Ok(m) => m,
        Err(_) => {
            // Echo the id back if we have one.
            let id = if pkt.len() >= 2 { u16::from_be_bytes([pkt[0], pkt[1]]) } else { 0 };
            let m = Message::error_msg(id, hickory_proto::op::OpCode::Query, ResponseCode::FormErr);
            return m.to_vec().unwrap_or_default();
        }
    };
    let max_udp = if proto == Proto::Udp {
        req.edns.as_ref().map(|e| e.max_payload() as usize).unwrap_or(512).max(512)
    } else {
        max_udp
    };
    if req.metadata.message_type != MessageType::Query || req.queries.len() != 1 {
        let m = Message::error_msg(req.metadata.id, req.metadata.op_code, ResponseCode::NotImp);
        return m.to_vec().unwrap_or_default();
    }
    let q = req.queries[0].clone();
    let domain = normalize_domain(&q.name().to_ascii());
    let qtype = q.query_type();

    // Find the querying process concurrently with resolution.
    let who = {
        let app = app.clone();
        tokio::spawn(async move { attribute(&app, peer, proto).await })
    };

    let mode = app.cfg.read().block_response;
    let mut event = DnsEvent {
        time: now(),
        domain: domain.clone(),
        qtype: qtype_name(qtype),
        blocked: false,
        reason: String::new(),
        answers: vec![],
        cached: false,
        upstream: None,
        latency_ms: 0,
        process: None,
    };

    let resp = if let Some(d) = decide(app, &domain).filter(|d| d.blocked()) {
        event.blocked = true;
        event.reason = d.reason;
        blocked_response(&req, mode)
    } else {
        match resolve(app, &req, &domain, qtype, &mut event).await {
            Some(r) => r,
            None => {
                app.counters.lock().dns_failed += 1;
                event.reason = "todos os servidores DNS falharam".into();
                Message::error_msg(req.metadata.id, req.metadata.op_code, ResponseCode::ServFail)
            }
        }
    };

    event.latency_ms = start.elapsed().as_millis() as u32;
    event.process = tokio::time::timeout(Duration::from_millis(300), who).await.ok().and_then(|r| r.ok()).flatten();
    {
        let mut c = app.counters.lock();
        c.dns_queries += 1;
        if event.blocked {
            c.dns_blocked += 1;
        }
        if event.cached {
            c.dns_cached += 1;
        }
    }
    app.count_minute(event.blocked);
    debug!("dns {} {} blocked={} {}", event.qtype, domain, event.blocked, event.reason);
    app.push_event(event);
    encode(resp, max_udp)
}

/// Resolves through cache/upstreams, applies CNAME filtering and learns IPs.
async fn resolve(app: &Arc<App>, req: &Message, domain: &str, qtype: RecordType, ev: &mut DnsEvent) -> Option<Message> {
    let key = (domain.to_string(), u16::from(qtype));
    // Cache.
    if let Some(e) = app.dns_cache.lock().get(&key) {
        let now_i = Instant::now();
        if e.expires > now_i {
            let elapsed = now_i.duration_since(e.stored).as_secs() as u32;
            let mut m = e.msg.clone();
            m.metadata.id = req.metadata.id;
            for r in m.answers.iter_mut().chain(m.authorities.iter_mut()) {
                r.ttl = r.ttl.saturating_sub(elapsed).max(1);
            }
            m.queries = req.queries.clone();
            ev.cached = true;
            ev.answers = answers_of(&m);
            return Some(m);
        }
    }

    // Upstream: send a clean query with our own EDNS.
    let mut up_q = Message::query();
    up_q.metadata.recursion_desired = true;
    up_q.queries = req.queries.clone();
    let mut edns = hickory_proto::op::Edns::new();
    edns.set_max_payload(1232);
    up_q.edns = Some(edns);
    let mut packet = up_q.to_vec().ok()?;

    let upstreams = app.cfg.read().upstreams.clone();
    let client = app.http_doh.read().clone();
    let mut answer = None;
    for up in &upstreams {
        if up.kind == portward_core::config::UpstreamKind::Doh {
            // RFC 8484: use id 0 for cache friendliness.
            packet[0] = 0;
            packet[1] = 0;
        } else {
            packet[0..2].copy_from_slice(&up_q.metadata.id.to_be_bytes());
        }
        match upstream::query(&client, up, &packet).await {
            Ok(bytes) => match Message::from_vec(&bytes) {
                Ok(m) => {
                    ev.upstream = Some(up.name.clone());
                    *app.last_upstream.write() = Some(up.name.clone());
                    answer = Some(m);
                    break;
                }
                Err(e) => warn!("upstream {} sent garbage: {e}", up.name),
            },
            Err(e) => warn!("upstream {} failed: {e:#}", up.name),
        }
    }
    let mut m = answer?;

    // CNAME cloaking: check every alias in the chain.
    let filter_cname = app.cfg.read().filter_cname;
    if filter_cname {
        for r in &m.answers {
            if let RData::CNAME(c) = &r.data {
                let target = normalize_domain(&c.0.to_ascii());
                if let Some(d) = decide(app, &target).filter(|d| d.blocked()) {
                    ev.blocked = true;
                    ev.reason = format!("CNAME {target}: {}", d.reason);
                    ev.answers = answers_of(&m);
                    return Some(blocked_response(req, app.cfg.read().block_response));
                }
            }
        }
    }

    // Learn IP → domain for the connection monitor.
    {
        let t = now();
        let mut map = app.ip_domains.lock();
        for r in &m.answers {
            match &r.data {
                RData::A(a) => {
                    map.insert(a.0.into(), (domain.to_string(), t));
                }
                RData::AAAA(a) => {
                    map.insert(a.0.into(), (domain.to_string(), t));
                }
                _ => {}
            }
        }
        if map.len() > 200_000 {
            map.retain(|_, (_, ts)| t - *ts < 6 * 3600);
        }
    }

    // Cache (positive: min TTL, negative: SOA minimum or 60s).
    let ttl = m
        .answers
        .iter()
        .map(|r| r.ttl)
        .min()
        .unwrap_or_else(|| m.authorities.iter().map(|r| r.ttl).min().unwrap_or(60))
        .clamp(10, 3600);
    if matches!(m.metadata.response_code, ResponseCode::NoError | ResponseCode::NXDomain) {
        let mut c = app.dns_cache.lock();
        if c.len() > 50_000 {
            let n = Instant::now();
            c.retain(|_, e| e.expires > n);
        }
        let n = Instant::now();
        c.insert(key, CacheEntry { msg: m.clone(), expires: n + Duration::from_secs(ttl as u64), stored: n });
    }

    m.metadata.id = req.metadata.id;
    m.metadata.recursion_available = true;
    m.queries = req.queries.clone();
    // Keep EDNS only if the client used it.
    if req.edns.is_none() {
        m.edns = None;
    }
    ev.answers = answers_of(&m);
    Some(m)
}

/// Used by the self-test: builds a raw query packet.
#[allow(dead_code)]
pub fn build_query(name: &str, t: RecordType) -> Vec<u8> {
    let mut m = Message::query();
    m.metadata.recursion_desired = true;
    m.add_query(hickory_proto::op::Query::query(Name::from_ascii(name).unwrap(), t));
    m.to_vec().unwrap()
}
