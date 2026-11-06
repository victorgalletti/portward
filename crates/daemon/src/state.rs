//! Shared daemon state.

use crate::store::Store;
use parking_lot::{Mutex, RwLock};
use portward_core::model::{Connection, Counters, CountItem, DnsEvent, FilterListStatus, MinuteBucket};
use portward_core::{Config, FilterSet, RuleSet};
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub type FlowKey = (crate::procinfo::Proto, std::net::SocketAddr, std::net::SocketAddr);

/// Traffic counters for one local/remote address pair, from packet capture.
#[derive(Debug, Default, Clone)]
pub struct Flow {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub last: i64,
    /// TLS server name seen in the ClientHello.
    pub sni: Option<String>,
}

/// One captured packet, summarised (Wireshark-style list entry).
#[derive(Debug, Clone, serde::Serialize)]
pub struct PacketRec {
    pub id: u64,
    /// Unix time with milliseconds.
    pub t: f64,
    pub iface: String,
    pub outbound: bool,
    pub proto: &'static str,
    pub local: String,
    pub remote: String,
    pub len: u32,
    pub ttl: u8,
    pub flags: String,
    pub info: String,
    /// First bytes of the IP packet, hex.
    pub hex: String,
}

pub struct CacheEntry {
    pub msg: hickory_proto::op::Message,
    pub expires: Instant,
    pub stored: Instant,
}

pub struct App {
    pub cfg: RwLock<Config>,
    pub cfg_path: PathBuf,
    pub data_dir: PathBuf,
    pub token: String,
    pub root: bool,
    pub started: i64,

    pub rules: RwLock<RuleSet>,
    pub rule_errors: RwLock<Vec<String>>,
    pub filters: RwLock<Arc<FilterSet>>,
    pub list_status: RwLock<Vec<FilterListStatus>>,
    pub lists_changed: Notify,
    pub force_list_update: Mutex<bool>,

    pub http_doh: RwLock<reqwest::Client>,
    pub http_plain: reqwest::Client,
    pub dns_cache: Mutex<HashMap<(String, u16), CacheEntry>>,
    /// Remote IP → (domain, unix time) learned from DNS answers.
    pub ip_domains: Mutex<HashMap<IpAddr, (String, i64)>>,
    pub dns_events: Mutex<VecDeque<DnsEvent>>,
    pub dns_ok: RwLock<bool>,
    pub last_upstream: RwLock<Option<String>>,
    /// Local UDP port → pid, refreshed by the connection monitor.
    pub port_owner: RwLock<HashMap<u16, i32>>,

    pub conns: Mutex<HashMap<String, Connection>>,
    pub counters: Mutex<Counters>,
    pub minutes: Mutex<VecDeque<MinuteBucket>>,
    pub top_blocked: Mutex<HashMap<String, u64>>,
    pub top_domains: Mutex<HashMap<String, u64>>,

    pub store: Store,
    pub pf_active: RwLock<bool>,
    pub pf_changed: Notify,
    pub system_dns_managed: RwLock<bool>,
    pub warnings: RwLock<Vec<String>>,

    pub geo: crate::geo::Geo,
    pub flows: Mutex<HashMap<FlowKey, Flow>>,
    /// Local IP → interface name.
    pub local_ips: RwLock<HashMap<IpAddr, String>>,
    pub interfaces: RwLock<Vec<portward_core::model::InterfaceInfo>>,
    pub capture_ifaces: RwLock<Vec<String>>,
    pub recheck_ip: Notify,
    /// app path ("" = everything) → last hour of 10 s traffic buckets.
    pub traffic: Mutex<HashMap<String, VecDeque<portward_core::model::TrafficBucket>>>,
    pub packets: Mutex<VecDeque<PacketRec>>,
    pub packet_seq: std::sync::atomic::AtomicU64,
    pub locations: Mutex<HashMap<String, portward_core::model::Location>>,
    pub shutdown: Notify,
}

impl App {
    pub fn flush_cache(&self) {
        self.dns_cache.lock().clear();
    }

    /// Re-parses rules from the config and signals the packet filter.
    pub fn reload_rules(&self) {
        let (rs, errs) = RuleSet::parse(&self.cfg.read().rules);
        *self.rules.write() = rs;
        *self.rule_errors.write() = errs;
        self.flush_cache();
        self.pf_changed.notify_one();
    }

    pub fn save_config(&self) -> anyhow::Result<()> {
        self.cfg.read().save(&self.cfg_path)
    }

    pub fn count_minute(&self, blocked: bool) {
        let m = now() / 60 * 60;
        let mut mins = self.minutes.lock();
        if mins.back().map(|b| b.minute) != Some(m) {
            mins.push_back(MinuteBucket { minute: m, queries: 0, blocked: 0 });
            while mins.len() > 24 * 60 {
                mins.pop_front();
            }
        }
        let b = mins.back_mut().unwrap();
        b.queries += 1;
        if blocked {
            b.blocked += 1;
        }
    }

    pub fn push_event(&self, ev: DnsEvent) {
        {
            let key = ev.domain.clone();
            if ev.blocked {
                *self.top_blocked.lock().entry(key).or_default() += 1;
            } else {
                *self.top_domains.lock().entry(key).or_default() += 1;
            }
        }
        self.store.record_dns(&ev);
        let mut q = self.dns_events.lock();
        q.push_back(ev);
        while q.len() > 5000 {
            q.pop_front();
        }
    }
}

impl App {
    pub fn add_traffic(&self, app_path: &str, bin: u64, bout: u64, new_conn: bool) {
        if bin == 0 && bout == 0 && !new_conn {
            return;
        }
        let t = now() / 10 * 10;
        let mut tr = self.traffic.lock();
        for key in ["", app_path] {
            let q = tr.entry(key.to_string()).or_default();
            if q.back().map(|b| b.t) != Some(t) {
                q.push_back(portward_core::model::TrafficBucket { t, ..Default::default() });
                while q.len() > 360 {
                    q.pop_front();
                }
            }
            let b = q.back_mut().unwrap();
            b.bytes_in += bin;
            b.bytes_out += bout;
            if new_conn {
                b.new_connections += 1;
            }
        }
        if tr.len() > 2000 {
            let cutoff = t - 3600;
            tr.retain(|k, q| k.is_empty() || q.back().map(|b| b.t > cutoff).unwrap_or(false));
        }
    }
}

pub fn top(map: &HashMap<String, u64>, n: usize) -> Vec<CountItem> {
    let mut v: Vec<_> = map.iter().map(|(k, c)| CountItem { key: k.clone(), count: *c }).collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    v.truncate(n);
    v
}
