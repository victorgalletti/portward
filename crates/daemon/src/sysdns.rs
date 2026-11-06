//! Points the system DNS of every network service at Portward and restores the
//! previous settings on shutdown (or with `portwardd restore-dns`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use tracing::{info, warn};

const SAVED: &str = "saved-dns.json";

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    /// service → previous servers (empty = DHCP / automatic).
    services: BTreeMap<String, Vec<String>>,
}

fn networksetup(args: &[&str]) -> Result<String> {
    let out = Command::new("/usr/sbin/networksetup").args(args).output().context("networksetup")?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn services() -> Result<Vec<String>> {
    Ok(networksetup(&["-listallnetworkservices"])?
        .lines()
        .skip(1) // "An asterisk (*) denotes that a network service is disabled."
        .filter(|l| !l.starts_with('*') && !l.trim().is_empty())
        .map(str::to_string)
        .collect())
}

fn current(service: &str) -> Result<Vec<String>> {
    let out = networksetup(&["-getdnsservers", service])?;
    if out.contains("aren't any") || out.contains("Error") {
        return Ok(vec![]);
    }
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

pub fn flush_system_cache() {
    let _ = Command::new("/usr/bin/dscacheutil").arg("-flushcache").status();
    let _ = Command::new("/usr/bin/killall").args(["-HUP", "mDNSResponder"]).status();
}

/// Sets 127.0.0.1 on all services. Existing saved settings (from a crash) are
/// kept, so the original configuration is never lost.
pub fn take_over(data_dir: &Path) -> Result<()> {
    let path = data_dir.join(SAVED);
    let mut saved: Saved = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    for svc in services()? {
        let cur = current(&svc)?;
        if !saved.services.contains_key(&svc) {
            let prev: Vec<String> = cur.into_iter().filter(|s| s != "127.0.0.1" && s != "::1").collect();
            saved.services.insert(svc.clone(), prev);
        }
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&saved)?)?;
    for svc in saved.services.keys() {
        networksetup(&["-setdnsservers", svc, "127.0.0.1"])?;
    }
    flush_system_cache();
    info!("system DNS now points to Portward on {} services", saved.services.len());
    Ok(())
}

pub fn restore(data_dir: &Path) -> Result<()> {
    let path = data_dir.join(SAVED);
    let Ok(bytes) = std::fs::read(&path) else {
        info!("no saved DNS settings to restore");
        return Ok(());
    };
    let saved: Saved = serde_json::from_slice(&bytes)?;
    for (svc, servers) in &saved.services {
        let mut args = vec!["-setdnsservers", svc.as_str()];
        if servers.is_empty() {
            args.push("Empty");
        } else {
            args.extend(servers.iter().map(String::as_str));
        }
        if let Err(e) = networksetup(&args) {
            warn!("restoring DNS of {svc}: {e:#}");
        }
    }
    std::fs::remove_file(&path)?;
    flush_system_cache();
    info!("system DNS settings restored");
    Ok(())
}
