//! Types exchanged between the daemon and the UI over the local API.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: i32,
    pub name: String,
    /// Executable path.
    pub path: String,
    /// Grouping key: the outermost `.app` bundle, or the executable path.
    pub app_path: String,
    pub uid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// Stable id: protocol + local + remote + pid.
    pub id: String,
    pub protocol: String,
    pub local: String,
    pub remote: String,
    pub remote_ip: String,
    pub remote_port: u16,
    /// Domain the remote IP was resolved from through our DNS, if known.
    pub domain: Option<String>,
    /// `internet`, `lan`, `localhost`, `multicast`.
    pub scope: String,
    pub state: String,
    pub inbound: bool,
    pub process: ProcessInfo,
    /// Unix seconds.
    pub first_seen: i64,
    pub last_seen: i64,
    pub active: bool,
    /// Set when the remote IP is blocked by an IP rule (pf drops it).
    pub blocked: bool,
    pub reason: Option<String>,
    #[serde(default)]
    pub bytes_in: u64,
    #[serde(default)]
    pub bytes_out: u64,
    /// Where the remote server is (ISO code + name) and who runs it.
    #[serde(default)]
    pub geo: Option<GeoInfo>,
    /// Local interface the connection leaves through (en0, utun4…).
    #[serde(default)]
    pub interface: Option<String>,
    /// The IP address of yours that the remote server sees.
    #[serde(default)]
    pub exposed_ip: Option<String>,
    #[serde(default)]
    pub via_vpn: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeoInfo {
    pub country: Option<String>,
    pub country_name: Option<String>,
    pub asn: Option<u32>,
    pub org: Option<String>,
}

/// A network interface and the public identity it gives you.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceInfo {
    pub name: String,
    /// `wifi`, `ethernet`, `vpn`, `other`.
    pub kind: String,
    pub label: String,
    pub local_ips: Vec<String>,
    pub public_ipv4: Option<String>,
    pub public_ipv6: Option<String>,
    pub geo_v4: Option<GeoInfo>,
    pub geo_v6: Option<GeoInfo>,
    pub checked: Option<i64>,
    pub error: Option<String>,
    pub capturing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsEvent {
    pub time: i64,
    pub domain: String,
    pub qtype: String,
    pub blocked: bool,
    pub reason: String,
    pub answers: Vec<String>,
    pub cached: bool,
    pub upstream: Option<String>,
    pub latency_ms: u32,
    /// The process that sent the query. On macOS most apps resolve through
    /// mDNSResponder, so this is often the system resolver itself.
    pub process: Option<ProcessInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSummary {
    pub path: String,
    pub name: String,
    pub active_connections: usize,
    pub total_connections: usize,
    pub blocked_connections: usize,
    pub domains: Vec<String>,
    pub last_seen: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Counters {
    pub dns_queries: u64,
    pub dns_blocked: u64,
    pub dns_cached: u64,
    pub dns_failed: u64,
    pub connections_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinuteBucket {
    /// Unix seconds, start of the minute.
    pub minute: i64,
    pub queries: u32,
    pub blocked: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountItem {
    pub key: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterListStatus {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub url: String,
    pub enabled: bool,
    pub domains: usize,
    pub updated: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub mode: crate::config::Mode,
    pub capture_interfaces: Vec<String>,
    pub version: String,
    pub started: i64,
    pub running_as_root: bool,
    pub dns_listen: String,
    pub dns_ok: bool,
    pub system_dns_managed: bool,
    pub pf_active: bool,
    pub paused: bool,
    pub blocked_domains: usize,
    pub last_upstream: Option<String>,
    pub counters: Counters,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stats {
    pub per_minute: Vec<MinuteBucket>,
    pub top_blocked: Vec<CountItem>,
    pub top_domains: Vec<CountItem>,
    pub top_apps: Vec<CountItem>,
    #[serde(default)]
    pub countries: Vec<CountItem>,
    #[serde(default)]
    pub orgs: Vec<CountItem>,
    #[serde(default)]
    pub bytes_in: u64,
    #[serde(default)]
    pub bytes_out: u64,
    #[serde(default)]
    pub remote_servers: usize,
}
