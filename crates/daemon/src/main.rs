//! portwardd — the Portward privacy filter daemon for macOS.

mod api;
mod capture;
mod dns;
mod filters;
mod geo;
mod monitor;
mod netif;
mod packet;
mod pf;
mod procinfo;
mod state;
mod store;
mod sysdns;
mod upstream;

use clap::{Parser, Subcommand};
use parking_lot::{Mutex, RwLock};
use portward_core::{Config, RuleSet};
use state::{now, App};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "portwardd", version, about = "Portward privacy filter daemon for macOS")]
struct Cli {
    /// Data directory (config, filter lists, history, API token).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the daemon.
    Run {
        /// Override the DNS listen address (e.g. 127.0.0.1:5300 for testing without root).
        #[arg(long)]
        dns_listen: Option<String>,
        /// Override the API listen address.
        #[arg(long)]
        api_listen: Option<String>,
        /// Point the system DNS of all network services at Portward while running.
        #[arg(long)]
        manage_system_dns: bool,
    },
    /// Restore the system DNS settings saved by --manage-system-dns.
    RestoreDns,
    /// Print where data lives and the API token path.
    Info,
}

fn default_data_dir(root: bool) -> PathBuf {
    if root {
        PathBuf::from("/Library/Application Support/Portward")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join("Library/Application Support/Portward")
    }
}

fn load_or_create_token(dir: &std::path::Path, root: bool) -> anyhow::Result<String> {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("api-token");
    if let Ok(t) = std::fs::read_to_string(&p) {
        if t.trim().len() >= 32 {
            return Ok(t.trim().to_string());
        }
    }
    let mut bytes = [0u8; 24];
    std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut bytes))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(&p, &token)?;
    // root:admin 0640 — readable by administrators (the UI user), nobody else.
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(if root { 0o640 } else { 0o600 }))?;
    if root {
        let c = std::ffi::CString::new(p.to_string_lossy().as_bytes())?;
        unsafe { libc::chown(c.as_ptr(), 0, 80) };
    }
    Ok(token)
}

pub fn sysdns_flush(app: &App) {
    if *app.system_dns_managed.read() {
        std::thread::spawn(sysdns::flush_system_cache);
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let cli = Cli::parse();
    let root = unsafe { libc::geteuid() } == 0;
    let data_dir = cli.data_dir.clone().unwrap_or_else(|| default_data_dir(root));
    std::fs::create_dir_all(&data_dir)?;

    match cli.cmd {
        Cmd::RestoreDns => sysdns::restore(&data_dir),
        Cmd::Info => {
            println!("data dir:  {}", data_dir.display());
            println!("config:    {}", data_dir.join("config.json").display());
            println!("api token: {}", data_dir.join("api-token").display());
            println!("root:      {root}");
            Ok(())
        }
        Cmd::Run { dns_listen, api_listen, manage_system_dns } => {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
            rt.block_on(run(data_dir, root, dns_listen, api_listen, manage_system_dns))
        }
    }
}

async fn run(
    data_dir: PathBuf,
    root: bool,
    dns_listen: Option<String>,
    api_listen: Option<String>,
    manage_system_dns: bool,
) -> anyhow::Result<()> {
    let cfg_path = data_dir.join("config.json");
    let mut cfg = Config::load_or_default(&cfg_path)?;
    if let Some(d) = dns_listen {
        cfg.dns_listen = d;
    }
    if let Some(a) = api_listen {
        cfg.api_listen = a;
    }
    if !cfg_path.exists() {
        cfg.save(&cfg_path)?;
    }
    let token = load_or_create_token(&data_dir, root)?;
    let (rules, rule_errors) = RuleSet::parse(&cfg.rules);
    let store = store::Store::open(&data_dir.join("history.sqlite"))?;
    store.prune(cfg.history_days);

    let app = Arc::new(App {
        http_doh: RwLock::new(upstream::build_client(&cfg.upstreams)),
        http_plain: reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("portward/", env!("CARGO_PKG_VERSION")))
            .build()?,
        cfg: RwLock::new(cfg.clone()),
        cfg_path,
        data_dir: data_dir.clone(),
        token,
        root,
        started: now(),
        rules: RwLock::new(rules),
        rule_errors: RwLock::new(rule_errors),
        filters: Default::default(),
        list_status: Default::default(),
        lists_changed: Default::default(),
        force_list_update: Mutex::new(false),
        dns_cache: Default::default(),
        ip_domains: Default::default(),
        dns_events: Default::default(),
        dns_ok: RwLock::new(false),
        last_upstream: Default::default(),
        port_owner: Default::default(),
        conns: Default::default(),
        counters: Default::default(),
        minutes: Default::default(),
        top_blocked: Default::default(),
        top_domains: Default::default(),
        store,
        pf_active: RwLock::new(false),
        pf_changed: Default::default(),
        system_dns_managed: RwLock::new(false),
        warnings: Default::default(),
        geo: Default::default(),
        flows: Default::default(),
        local_ips: Default::default(),
        interfaces: Default::default(),
        capture_ifaces: Default::default(),
        recheck_ip: Default::default(),
        shutdown: Default::default(),
    });
    let filter_mode = cfg.mode == portward_core::config::Mode::Filter;
    info!("mode: {:?}", cfg.mode);
    if !filter_mode && root {
        // Coming from filter mode (or a crash): make sure the original DNS is back.
        let dir = data_dir.clone();
        let _ = tokio::task::spawn_blocking(move || sysdns::restore(&dir)).await;
    }
    if !root {
        app.warnings.write().push(
            "Rodando sem root: só os seus próprios processos aparecem.".into(),
        );
    }

    // DNS server (filter mode only).
    let dns_addr: SocketAddr = cfg.dns_listen.parse()?;
    let a = app.clone();
    let dns_task = tokio::spawn(async move {
        if a.cfg.read().mode != portward_core::config::Mode::Filter {
            return;
        }
        if let Err(e) = dns::serve(a.clone(), dns_addr).await {
            error!("DNS server failed: {e:#}");
            *a.dns_ok.write() = false;
            a.warnings.write().push(format!("O servidor DNS não iniciou em {dns_addr}: {e:#}"));
        }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Point the system at us only once the server is really up.
    if filter_mode && manage_system_dns && *app.dns_ok.read() {
        if dns_addr.port() != 53 || !dns_addr.ip().is_loopback() {
            warn!("--manage-system-dns needs dns_listen on 127.0.0.1:53");
        } else {
            let dir = data_dir.clone();
            match tokio::task::spawn_blocking(move || sysdns::take_over(&dir)).await? {
                Ok(()) => *app.system_dns_managed.write() = true,
                Err(e) => app.warnings.write().push(format!("Não foi possível alterar o DNS do sistema: {e:#}")),
            }
        }
    }

    if filter_mode {
        tokio::spawn(filters::run(app.clone()));
    }
    tokio::spawn(geo::run(app.clone()));
    tokio::spawn(netif::run(app.clone()));
    tokio::spawn(capture::run(app.clone()));
    tokio::spawn(monitor::run(app.clone()));
    let pf_token = if filter_mode { pf::run(app.clone()).await } else { None };
    {
        let a = app.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                a.store.prune(a.cfg.read().history_days);
            }
        });
    }

    let api_addr: SocketAddr = cfg.api_listen.parse()?;
    let listener = tokio::net::TcpListener::bind(api_addr).await?;
    info!("API listening on http://{api_addr}");
    let api = axum::serve(listener, api::router(app.clone()));

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        r = api => { if let Err(e) = r { error!("API server: {e}"); } }
        _ = tokio::signal::ctrl_c() => info!("interrupted"),
        _ = term.recv() => info!("terminated"),
        _ = app.shutdown.notified() => info!("restarting to switch mode"),
    }

    // Clean shutdown: give the network back exactly as we found it.
    info!("shutting down");
    if *app.system_dns_managed.read() {
        if let Err(e) = sysdns::restore(&data_dir) {
            error!("restoring system DNS: {e:#}");
        }
    }
    if let Some(t) = pf_token {
        pf::stop(&t);
    }
    dns_task.abort();
    // Capture threads block in libpcap; don't wait for them.
    std::process::exit(0)
}
