//! Downloads, caches and compiles the enabled filter lists.

use crate::state::{now, App};
use portward_core::filterlist::parse_list;
use portward_core::model::FilterListStatus;
use portward_core::FilterSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

fn list_path(app: &App, id: &str) -> std::path::PathBuf {
    app.data_dir.join("lists").join(format!("{id}.txt"))
}

fn mtime(p: &std::path::Path) -> Option<i64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Compiles all enabled lists that are on disk into a new FilterSet.
pub fn rebuild(app: &App) {
    let cfg = app.cfg.read().clone();
    let mut set = FilterSet::new();
    let mut statuses = vec![];
    let old = app.list_status.read().clone();
    for l in &cfg.filter_lists {
        let p = list_path(app, &l.id);
        let mut st = FilterListStatus {
            id: l.id.clone(),
            name: l.name.clone(),
            category: l.category.clone(),
            description: l.description.clone(),
            url: l.url.clone(),
            enabled: l.enabled,
            domains: 0,
            updated: mtime(&p),
            error: old.iter().find(|o| o.id == l.id).and_then(|o| o.error.clone()),
        };
        if l.enabled {
            if let Ok(body) = std::fs::read_to_string(&p) {
                st.domains = set.add_list(&l.name, parse_list(&body));
            }
        }
        statuses.push(st);
    }
    info!("filter lists compiled: {} unique domains", set.len());
    *app.filters.write() = Arc::new(set);
    *app.list_status.write() = statuses;
    app.flush_cache();
}

/// Downloads enabled lists that are missing or older than the refresh interval
/// (or all enabled lists when `force`). Returns true if anything changed.
pub async fn update(app: &App, force: bool) -> bool {
    let cfg = app.cfg.read().clone();
    let max_age = cfg.filterlist_refresh_hours as i64 * 3600;
    let _ = std::fs::create_dir_all(app.data_dir.join("lists"));
    let mut changed = false;
    for l in cfg.filter_lists.iter().filter(|l| l.enabled) {
        let p = list_path(app, &l.id);
        let fresh = mtime(&p).map(|t| now() - t < max_age).unwrap_or(false);
        if fresh && !force {
            continue;
        }
        let result = async {
            let resp = app.http_plain.get(&l.url).send().await?.error_for_status()?;
            let body = resp.text().await?;
            let n = parse_list(&body).len();
            if n == 0 {
                anyhow::bail!("lista vazia ou em formato desconhecido");
            }
            let tmp = p.with_extension("tmp");
            std::fs::write(&tmp, body)?;
            std::fs::rename(&tmp, &p)?;
            anyhow::Ok(n)
        }
        .await;
        let err = match result {
            Ok(n) => {
                info!("downloaded filter list {} ({} domains)", l.name, n);
                changed = true;
                None
            }
            Err(e) => {
                warn!("filter list {} failed: {e:#}", l.name);
                Some(format!("{e:#}"))
            }
        };
        let mut st = app.list_status.write();
        if let Some(s) = st.iter_mut().find(|s| s.id == l.id) {
            s.error = err;
        }
    }
    changed
}

pub async fn run(app: Arc<App>) {
    rebuild(&app);
    // Give the DNS server a moment to come up before downloading.
    tokio::time::sleep(Duration::from_secs(2)).await;
    loop {
        if update(&app, false).await {
            rebuild(&app);
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(3600)) => {}
            _ = app.lists_changed.notified() => {
                let force = std::mem::take(&mut *app.force_list_update.lock());
                update(&app, force).await;
                rebuild(&app);
            }
        }
    }
}
