//! IP blocking with the macOS packet filter (pf).
//!
//! We load our rules into the sub-anchor `com.apple/portward`. The stock
//! /etc/pf.conf already evaluates `anchor "com.apple/*"`, so nothing in the
//! system configuration has to be edited, and removing our anchor restores the
//! original state exactly.

use crate::state::App;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;
use tracing::{info, warn};

const ANCHOR: &str = "com.apple/portward";
const TABLE: &str = "portward_block";

fn pfctl(args: &[&str], stdin: Option<&str>) -> Result<String> {
    let mut child = Command::new("/sbin/pfctl")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn pfctl")?;
    if let Some(s) = stdin {
        child.stdin.take().unwrap().write_all(s.as_bytes())?;
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        bail!("pfctl {:?}: {}", args, text.trim());
    }
    Ok(text)
}

/// Enables pf (reference counted through a token) and loads our anchor.
pub fn start() -> Result<String> {
    let out = pfctl(&["-E"], None)?;
    let token = out
        .lines()
        .find_map(|l| l.strip_prefix("Token : "))
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let rules = format!(
        "table <{TABLE}> persist\nblock drop out quick to <{TABLE}>\nblock drop in quick from <{TABLE}>\n"
    );
    pfctl(&["-a", ANCHOR, "-f", "-"], Some(&rules))?;
    info!("pf anchor {ANCHOR} loaded");
    Ok(token)
}

pub fn set_blocked(app: &App) -> Result<usize> {
    let nets = if app.cfg.read().paused { vec![] } else { app.rules.read().blocked_networks() };
    let file = app.data_dir.join("pf-table.txt");
    let body: String = nets.iter().map(|n| format!("{n}\n")).collect();
    std::fs::write(&file, body)?;
    let path = file.to_string_lossy().to_string();
    pfctl(&["-a", ANCHOR, "-t", TABLE, "-T", "replace", "-f", &path], None)?;
    // Kill existing states so already-open connections are cut, too.
    for n in &nets {
        let _ = pfctl(&["-k", "0.0.0.0/0", "-k", &n.to_string()], None);
    }
    Ok(nets.len())
}

pub fn stop(token: &str) {
    let _ = pfctl(&["-a", ANCHOR, "-F", "all"], None);
    if !token.is_empty() {
        let _ = pfctl(&["-X", token], None);
    }
    info!("pf anchor removed");
}

pub async fn run(app: Arc<App>) -> Option<String> {
    if !app.root || !app.cfg.read().enforce_ip_rules {
        return None;
    }
    let token = match tokio::task::spawn_blocking(start).await.ok()? {
        Ok(t) => t,
        Err(e) => {
            warn!("pf unavailable: {e:#}");
            app.warnings.write().push(format!("Firewall de pacotes (pf) indisponível: {e:#}"));
            return None;
        }
    };
    *app.pf_active.write() = true;
    let app2 = app.clone();
    tokio::spawn(async move {
        loop {
            let a = app2.clone();
            match tokio::task::spawn_blocking(move || set_blocked(&a)).await {
                Ok(Ok(n)) => info!("pf: {n} blocked networks"),
                Ok(Err(e)) => warn!("pf update failed: {e:#}"),
                Err(_) => {}
            }
            app2.pf_changed.notified().await;
        }
    });
    Some(token)
}
