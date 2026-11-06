//! Connection monitor: periodically snapshots every process's sockets and
//! turns them into per-app connection records.

use crate::procinfo::{self, Proto, SockEntry};
use crate::state::{now, App};
use portward_core::model::Connection;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use std::sync::Arc;
use std::time::Duration;

pub fn scope_of(ip: IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() {
                "localhost"
            } else if v4.is_multicast() || v4.is_broadcast() {
                "multicast"
            } else if v4.is_private() || v4.is_link_local() || v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64 {
                "lan"
            } else {
                "internet"
            }
        }
        IpAddr::V6(v6) => {
            let s = v6.segments()[0];
            if v6.is_loopback() {
                "localhost"
            } else if v6.is_multicast() {
                "multicast"
            } else if (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80 {
                "lan"
            } else {
                "internet"
            }
        }
    }
}

const KEEP_ENDED_SECS: i64 = 15 * 60;

pub async fn run(app: Arc<App>) {
    loop {
        let interval = app.cfg.read().monitor_interval_ms.max(250);
        let socks = tokio::task::spawn_blocking(procinfo::all_sockets).await.unwrap_or_default();
        scan(&app, socks);
        tokio::time::sleep(Duration::from_millis(interval)).await;
    }
}

fn scan(app: &App, socks: Vec<SockEntry>) {
    let t = now();
    // Port owner map for DNS query attribution (UDP sockets on any address).
    let mut owners = HashMap::new();
    let mut listening: HashSet<(Proto, u16)> = HashSet::new();
    for s in &socks {
        if s.proto == Proto::Udp {
            owners.insert(s.local.port(), s.pid);
        }
        if s.is_listener() {
            listening.insert((s.proto, s.local.port()));
        }
    }
    *app.port_owner.write() = owners;

    let rules = app.rules.read().clone();
    let paused = app.cfg.read().paused;
    let pf = *app.pf_active.read();
    let local_ips = app.local_ips.read().clone();
    let ifaces = app.interfaces.read().clone();
    let flows = app.flows.lock().clone();
    let ip_domains = app.ip_domains.lock();
    let mut conns = app.conns.lock();
    let mut seen = HashSet::new();
    let mut new_conns = vec![];

    // Which interface a local address belongs to, and what public IP that
    // interface shows to the world.
    let exposure = |local: IpAddr, remote: IpAddr| -> (Option<String>, Option<String>, bool) {
        let iface = local_ips.get(&local).cloned().or_else(|| {
            // Unbound sockets: assume the interface with a public IP of the same family.
            ifaces.iter().find(|i| i.kind != "vpn" && if remote.is_ipv4() { i.public_ipv4.is_some() } else { i.public_ipv6.is_some() }).map(|i| i.name.clone())
        });
        if scope_of(remote) != "internet" {
            return (iface, None, false);
        }
        let info = iface.as_ref().and_then(|n| ifaces.iter().find(|i| &i.name == n));
        let via_vpn = info.map(|i| i.kind == "vpn").unwrap_or(false);
        let exposed = if crate::geo::is_public(local) {
            Some(local.to_string())
        } else {
            info.and_then(|i| if remote.is_ipv4() { i.public_ipv4.clone() } else { i.public_ipv6.clone() })
        };
        (iface, exposed, via_vpn)
    };

    // Network extensions (antivirus web filters, VPN split tunnels) re-open
    // other apps' flows with the very same addresses. Credit those flows to the
    // real app and hide the extension's copy.
    let is_ext = |pid: i32| procinfo::process_info(pid).path.contains("/Library/SystemExtensions/");
    let owned_by_apps: HashSet<(Proto, std::net::SocketAddr, std::net::SocketAddr)> = socks
        .iter()
        .filter(|s| !s.is_listener() && !is_ext(s.pid))
        .map(|s| (s.proto, s.local, s.remote))
        .collect();

    for s in socks.iter().filter(|s| {
        !s.is_listener() && !s.remote.ip().is_unspecified() && !(is_ext(s.pid) && owned_by_apps.contains(&(s.proto, s.local, s.remote)))
    }) {
        let id = format!("{}|{}|{}|{}", s.proto.as_str(), s.local, s.remote, s.pid);
        seen.insert(id.clone());
        let flow = flows.get(&(s.proto, s.local, s.remote));
        if let Some(c) = conns.get_mut(&id) {
            c.last_seen = t;
            c.active = true;
            c.state = s.state.to_string();
            if let Some(f) = flow {
                app.add_traffic(&c.process.app_path, f.bytes_in.saturating_sub(c.bytes_in), f.bytes_out.saturating_sub(c.bytes_out), false);
                c.bytes_in = f.bytes_in;
                c.bytes_out = f.bytes_out;
                if let Some(sni) = &f.sni {
                    c.domain = Some(sni.clone());
                }
            }
            if c.domain.is_none() {
                c.domain = ip_domains.get(&s.remote.ip()).map(|(d, _)| d.clone());
            }
            if c.geo.as_ref().map(|g| g.country.is_none()).unwrap_or(true) && c.scope == "internet" {
                c.geo = app.geo.lookup(s.remote.ip());
            }
            if c.exposed_ip.is_none() && c.scope == "internet" {
                let (i, e, v) = exposure(s.local.ip(), s.remote.ip());
                c.interface = i;
                c.exposed_ip = e;
                c.via_vpn = v;
            }
            continue;
        }
        let rip = s.remote.ip();
        let (interface, exposed_ip, via_vpn) = exposure(s.local.ip(), rip);
        let decision = if paused { None } else { rules.check_ip(rip) };
        let blocked = pf && decision.as_ref().map(|d| d.blocked()).unwrap_or(false);
        let c = Connection {
            id: id.clone(),
            protocol: s.proto.as_str().to_string(),
            local: s.local.to_string(),
            remote: s.remote.to_string(),
            remote_ip: rip.to_string(),
            remote_port: s.remote.port(),
            domain: flow.and_then(|f| f.sni.clone()).or_else(|| ip_domains.get(&rip).map(|(d, _)| d.clone())),
            scope: scope_of(rip).to_string(),
            state: s.state.to_string(),
            inbound: listening.contains(&(s.proto, s.local.port())),
            process: procinfo::process_info(s.pid),
            first_seen: t,
            last_seen: t,
            active: true,
            blocked,
            reason: decision.map(|d| d.reason),
            bytes_in: flow.map(|f| f.bytes_in).unwrap_or(0),
            bytes_out: flow.map(|f| f.bytes_out).unwrap_or(0),
            geo: app.geo.lookup(rip),
            interface,
            exposed_ip,
            via_vpn,
        };
        app.add_traffic(&c.process.app_path, c.bytes_in, c.bytes_out, true);
        new_conns.push(c.clone());
        conns.insert(id, c);
    }
    drop(ip_domains);

    for c in conns.values_mut() {
        if c.active && !seen.contains(&c.id) {
            c.active = false;
            app.store.end_connection(c);
        }
    }
    conns.retain(|_, c| c.active || t - c.last_seen < KEEP_ENDED_SECS);
    if conns.len() > 20_000 {
        conns.retain(|_, c| c.active);
    }
    drop(conns);

    {
        let mut f = app.flows.lock();
        if f.len() > 5000 {
            f.retain(|_, fl| t - fl.last < 600);
        }
    }
    app.counters.lock().connections_seen += new_conns.len() as u64;
    for c in &new_conns {
        app.store.record_connection(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes() {
        assert_eq!(scope_of("8.8.8.8".parse().unwrap()), "internet");
        assert_eq!(scope_of("192.168.1.2".parse().unwrap()), "lan");
        assert_eq!(scope_of("127.0.0.1".parse().unwrap()), "localhost");
        assert_eq!(scope_of("fe80::1".parse().unwrap()), "lan");
        assert_eq!(scope_of("2606:4700::1111".parse().unwrap()), "internet");
    }
}
