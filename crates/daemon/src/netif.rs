//! Network interfaces and the public IP address each one shows to the world.
//!
//! The public address is learned by asking an IP echo service *through that
//! specific interface* (IP_BOUND_IF). With a split-tunnel VPN this gives two
//! answers: one for traffic that leaves through Wi-Fi, one for the tunnel.

use crate::geo::is_public;
use crate::state::{now, App};
use portward_core::model::InterfaceInfo;
use std::collections::HashMap;
use std::net::IpAddr;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tracing::debug;

pub struct RawIface {
    pub name: String,
    pub ips: Vec<IpAddr>,
}

fn link_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_link_local() || v.is_loopback(),
        IpAddr::V6(v) => (v.segments()[0] & 0xffc0) == 0xfe80 || v.is_loopback(),
    }
}

/// Interfaces that are up and have at least one routable address.
pub fn list() -> Vec<RawIface> {
    let Ok(devs) = pcap::Device::list() else { return vec![] };
    devs.into_iter()
        .filter(|d| d.flags.is_up() && d.flags.is_running() && !d.flags.is_loopback())
        .map(|d| RawIface { ips: d.addresses.iter().map(|a| a.addr).filter(|a| !link_local(a)).collect(), name: d.name })
        .filter(|d| !d.ips.is_empty())
        .collect()
}

/// `en0` → "Wi-Fi" etc., from `networksetup -listallhardwareports`.
fn hardware_ports() -> HashMap<String, String> {
    let out = Command::new("/usr/sbin/networksetup").arg("-listallhardwareports").output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    let mut map = HashMap::new();
    let mut port = None;
    for l in text.lines() {
        if let Some(p) = l.strip_prefix("Hardware Port: ") {
            port = Some(p.trim().to_string());
        } else if let Some(d) = l.strip_prefix("Device: ") {
            if let Some(p) = port.take() {
                map.insert(d.trim().to_string(), p);
            }
        }
    }
    map
}

fn classify(name: &str, ips: &[IpAddr], ports: &HashMap<String, String>) -> (String, String) {
    let tailscale = ips.iter().any(|ip| matches!(ip, IpAddr::V4(v) if v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64));
    if name.starts_with("utun") || name.starts_with("ipsec") || name.starts_with("ppp") || name.starts_with("tun") || name.starts_with("tap") {
        let label = if tailscale { "Tailscale".to_string() } else { format!("VPN ({name})") };
        return ("vpn".into(), label);
    }
    match ports.get(name) {
        Some(p) if p.contains("Wi-Fi") || p.contains("AirPort") => ("wifi".into(), p.clone()),
        Some(p) if p.contains("Ethernet") || p.contains("Thunderbolt") || p.contains("USB") => ("ethernet".into(), p.clone()),
        Some(p) => ("other".into(), p.clone()),
        None => ("other".into(), name.to_string()),
    }
}

async fn public_ip(iface: &str, url: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .interface(iface)
        .timeout(Duration::from_secs(6))
        .build()?;
    let ip = client.get(url).send().await?.error_for_status()?.text().await?;
    let ip = ip.trim().to_string();
    ip.parse::<IpAddr>()?;
    Ok(ip)
}

/// Refreshes interface list + local IP map; returns true if it changed.
pub fn refresh_local(app: &App) -> bool {
    let ifs = list();
    let mut map = HashMap::new();
    for i in &ifs {
        for ip in &i.ips {
            map.insert(*ip, i.name.clone());
        }
    }
    let changed = *app.local_ips.read() != map;
    if changed {
        *app.local_ips.write() = map;
    }
    changed
}

async fn refresh_public(app: &App) {
    let ports = tokio::task::spawn_blocking(hardware_ports).await.unwrap_or_default();
    let ifs = tokio::task::spawn_blocking(list).await.unwrap_or_default();
    let capturing = app.capture_ifaces.read().clone();
    let mut out = vec![];
    for i in ifs {
        let (kind, label) = classify(&i.name, &i.ips, &ports);
        let has_v4 = i.ips.iter().any(|ip| ip.is_ipv4());
        let has_v6 = i.ips.iter().any(|ip| ip.is_ipv6() && is_public(*ip));
        let mut info = InterfaceInfo {
            name: i.name.clone(),
            kind,
            label,
            local_ips: i.ips.iter().map(|ip| ip.to_string()).collect(),
            public_ipv4: None,
            public_ipv6: None,
            geo_v4: None,
            geo_v6: None,
            checked: Some(now()),
            error: None,
            capturing: capturing.contains(&i.name),
            location: None,
        };
        if has_v4 {
            match public_ip(&i.name, "https://api.ipify.org").await {
                Ok(ip) => {
                    info.geo_v4 = ip.parse().ok().and_then(|ip| app.geo.lookup(ip));
                    info.public_ipv4 = Some(ip);
                }
                Err(e) => {
                    debug!("public ipv4 via {}: {e:#}", i.name);
                    info.error = Some("sem saída IPv4 para a internet por esta interface".into());
                }
            }
        }
        if has_v6 {
            if let Ok(ip) = public_ip(&i.name, "https://api6.ipify.org").await {
                info.geo_v6 = ip.parse().ok().and_then(|ip| app.geo.lookup(ip));
                info.public_ipv6 = Some(ip);
            }
        }
        let ip = info.public_ipv4.clone().or(info.public_ipv6.clone());
        if let Some(ip) = ip {
            info.location = locate(app, &ip).await;
        }
        out.push(info);
    }
    // Most useful first: VPN, Wi-Fi, Ethernet; ones with a public IP first.
    out.sort_by_key(|i| (i.public_ipv4.is_none() && i.public_ipv6.is_none(), match i.kind.as_str() { "vpn" => 0, "wifi" => 1, "ethernet" => 2, _ => 3 }));
    *app.interfaces.write() = out;
}

pub async fn run(app: Arc<App>) {
    let mut last_public = 0;
    loop {
        let changed = refresh_local(&app);
        if changed || now() - last_public > 120 || app.interfaces.read().is_empty() {
            refresh_public(&app).await;
            last_public = now();
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
            _ = app.recheck_ip.notified() => { last_public = 0; }
        }
    }
}

/// Approximate (city-level) location of one of *your own* public IPs, from
/// ipinfo.io. Only your public addresses are sent, once per address.
async fn locate(app: &App, ip: &str) -> Option<portward_core::model::Location> {
    if let Some(l) = app.locations.lock().get(ip) {
        return Some(l.clone());
    }
    let v: serde_json::Value = app
        .http_plain
        .get(format!("https://ipinfo.io/{ip}/json"))
        .header("accept", "application/json")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())?;
    let (lat, lon) = v.get("loc")?.as_str()?.split_once(',')?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let loc = portward_core::model::Location {
        city: s("city"),
        region: s("region"),
        country: s("country"),
        lat: lat.trim().parse().ok()?,
        lon: lon.trim().parse().ok()?,
        org: s("org"),
    };
    app.locations.lock().insert(ip.to_string(), loc.clone());
    Some(loc)
}
