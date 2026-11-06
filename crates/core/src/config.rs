use serde::{Deserialize, Serialize};
use std::path::Path;

/// What the daemon does.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Only watch: no DNS change, no firewall. Traffic is observed passively.
    #[default]
    Monitor,
    /// Watch and block: system DNS goes through Portward, IP rules go to pf.
    Filter,
}

/// How a blocked DNS query is answered.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlockResponse {
    /// Answer with NXDOMAIN ("this domain does not exist").
    Nxdomain,
    /// Answer A/AAAA queries with 0.0.0.0 / :: (fast failure in browsers).
    ZeroIp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamKind {
    /// DNS-over-HTTPS (RFC 8484), e.g. `https://cloudflare-dns.com/dns-query`.
    Doh,
    /// Plain DNS over UDP (falls back to TCP on truncation), e.g. `1.1.1.1:53`.
    Udp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Upstream {
    pub name: String,
    pub kind: UpstreamKind,
    /// URL for DoH, `ip:port` for UDP.
    pub address: String,
    /// For DoH: IPs of the DoH server, so we never depend on DNS to reach it.
    #[serde(default)]
    pub bootstrap: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FilterListSource {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub url: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    pub mode: Mode,
    /// Address the DNS server listens on.
    pub dns_listen: String,
    /// Address of the local control API.
    pub api_listen: String,
    pub upstreams: Vec<Upstream>,
    pub filter_lists: Vec<FilterListSource>,
    /// Global endpoint rules, Portmaster-style: `- ads.example.com`, `+ *.apple.com`,
    /// `- 10.0.0.0/8`. First matching rule wins. Evaluated before filter lists.
    pub rules: Vec<String>,
    /// Block subdomains of listed domains too (`x.ads.com` if `ads.com` is listed).
    pub filter_subdomains: bool,
    /// Also check every CNAME in an answer against the filter lists.
    pub filter_cname: bool,
    pub block_response: BlockResponse,
    /// Enforce IP rules with the macOS packet filter (requires root).
    pub enforce_ip_rules: bool,
    /// Hours between filter list refreshes.
    pub filterlist_refresh_hours: u64,
    /// Days of connection / query history to keep.
    pub history_days: u32,
    /// Seconds between connection monitor scans (milliseconds).
    pub monitor_interval_ms: u64,
    /// When paused, everything is allowed (lists and rules are ignored).
    pub paused: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Monitor,
            dns_listen: "127.0.0.1:53".into(),
            api_listen: "127.0.0.1:817".into(),
            upstreams: vec![
                Upstream {
                    name: "Quad9 (DoH)".into(),
                    kind: UpstreamKind::Doh,
                    address: "https://dns.quad9.net/dns-query".into(),
                    bootstrap: vec!["9.9.9.9".into(), "149.112.112.112".into()],
                },
                Upstream {
                    name: "Cloudflare (DoH)".into(),
                    kind: UpstreamKind::Doh,
                    address: "https://cloudflare-dns.com/dns-query".into(),
                    bootstrap: vec!["1.1.1.1".into(), "1.0.0.1".into()],
                },
                Upstream {
                    name: "Cloudflare (UDP)".into(),
                    kind: UpstreamKind::Udp,
                    address: "1.1.1.1:53".into(),
                    bootstrap: vec![],
                },
            ],
            filter_lists: default_filter_lists(),
            rules: vec![],
            filter_subdomains: true,
            filter_cname: true,
            block_response: BlockResponse::ZeroIp,
            enforce_ip_rules: true,
            filterlist_refresh_hours: 24,
            history_days: 7,
            monitor_interval_ms: 1000,
            paused: false,
        }
    }
}

fn src(id: &str, name: &str, cat: &str, desc: &str, url: &str, enabled: bool) -> FilterListSource {
    FilterListSource {
        id: id.into(),
        name: name.into(),
        category: cat.into(),
        description: desc.into(),
        url: url.into(),
        enabled,
    }
}

/// Built-in filter list catalogue. All lists are public, well maintained and in
/// hosts / plain-domain / adblock-domain format.
pub fn default_filter_lists() -> Vec<FilterListSource> {
    vec![
        src("hagezi-pro", "HaGeZi Pro", "Anúncios e rastreadores",
            "Lista ampla de anúncios, rastreamento, telemetria e phishing.",
            "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/pro-onlydomains.txt", true),
        src("stevenblack", "StevenBlack Unified", "Anúncios e rastreadores",
            "Arquivo hosts consolidado de anúncios e malware.",
            "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts", false),
        src("hagezi-tif", "HaGeZi Threat Intelligence", "Malware",
            "Malware, phishing, golpes e mineração escondida.",
            "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/tif.mini-onlydomains.txt", true),
        src("urlhaus", "URLhaus Malware", "Malware",
            "Domínios que distribuem malware (abuse.ch URLhaus, via malware-filter).",
            "https://malware-filter.gitlab.io/malware-filter/urlhaus-filter-hosts.txt", true),
        src("hagezi-native-apple", "HaGeZi Apple Telemetry", "Telemetria",
            "Bloqueia telemetria e rastreamento nativos da Apple.",
            "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.apple-onlydomains.txt", false),
        src("hagezi-native-winoffice", "HaGeZi Microsoft Telemetry", "Telemetria",
            "Bloqueia telemetria da Microsoft e do Office.",
            "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/native.winoffice-onlydomains.txt", false),
        src("hagezi-gambling", "HaGeZi Gambling", "Conteúdo",
            "Sites de apostas.",
            "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/wildcard/gambling.mini-onlydomains.txt", false),
        src("stevenblack-porn", "StevenBlack Adult", "Conteúdo",
            "Sites de conteúdo adulto.",
            "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/porn-only/hosts", false),
        src("stevenblack-social", "StevenBlack Social", "Conteúdo",
            "Redes sociais.",
            "https://raw.githubusercontent.com/StevenBlack/hosts/master/alternates/social-only/hosts", false),
    ]
}

impl Config {
    pub fn load_or_default(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(path)?;
        let mut cfg: Config = serde_json::from_str(&data)?;
        // Keep the catalogue up to date: refresh built-in list metadata and add
        // new built-in lists, but keep the user's enabled/disabled choices.
        for d in default_filter_lists() {
            match cfg.filter_lists.iter_mut().find(|l| l.id == d.id) {
                Some(l) => {
                    let enabled = l.enabled;
                    *l = FilterListSource { enabled, ..d };
                }
                None => cfg.filter_lists.push(d),
            }
        }
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}
