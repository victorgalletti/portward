//! City/country and network owner (ASN) of IP addresses, from the free DB-IP
//! Lite databases (CC BY 4.0, https://db-ip.com). Downloaded once a month; all
//! lookups are local, no IP ever leaves the machine. The city database is
//! memory-mapped so only the pages that are looked up are read.

use crate::state::App;
use maxminddb::{Mmap, Reader};
use parking_lot::{Mutex, RwLock};
use portward_core::model::GeoInfo;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Default)]
pub struct Geo {
    city: RwLock<Option<Reader<Mmap>>>,
    asn: RwLock<Option<Reader<Vec<u8>>>>,
    cache: Mutex<HashMap<IpAddr, GeoInfo>>,
}

#[derive(Deserialize)]
struct CityRec {
    country: Option<Named>,
    city: Option<Named>,
    location: Option<Loc>,
}
#[derive(Deserialize)]
struct Named {
    iso_code: Option<String>,
    names: Option<BTreeMap<String, String>>,
}
#[derive(Deserialize)]
struct Loc {
    latitude: Option<f64>,
    longitude: Option<f64>,
}
fn name_of(n: &Named) -> Option<String> {
    n.names.as_ref().and_then(|m| m.get("pt-BR").or_else(|| m.get("en")).cloned())
}
#[derive(Deserialize)]
struct AsnRec {
    autonomous_system_number: Option<u32>,
    autonomous_system_organization: Option<String>,
}

pub fn is_public(ip: IpAddr) -> bool {
    crate::monitor::scope_of(ip) == "internet"
}

impl Geo {
    pub fn lookup(&self, ip: IpAddr) -> Option<GeoInfo> {
        if !is_public(ip) {
            return None;
        }
        if let Some(g) = self.cache.lock().get(&ip) {
            return Some(g.clone());
        }
        let mut g = GeoInfo::default();
        if let Some(r) = self.city.read().as_ref() {
            if let Ok(Some(rec)) = r.lookup(ip).and_then(|l| l.decode::<CityRec>()) {
                if let Some(c) = &rec.country {
                    g.country_name = name_of(c);
                    g.country = c.iso_code.clone();
                }
                g.city = rec.city.as_ref().and_then(name_of);
                if let Some(l) = rec.location {
                    g.lat = l.latitude;
                    g.lon = l.longitude;
                }
            }
        }
        if let Some(r) = self.asn.read().as_ref() {
            if let Ok(Some(rec)) = r.lookup(ip).and_then(|l| l.decode::<AsnRec>()) {
                g.asn = rec.autonomous_system_number;
                g.org = rec.autonomous_system_organization;
            }
        }
        if g == GeoInfo::default() {
            return None;
        }
        let mut c = self.cache.lock();
        if c.len() > 50_000 {
            c.clear();
        }
        c.insert(ip, g.clone());
        Some(g)
    }

    fn load(&self, dir: &Path) {
        // SAFETY: the file is only ever replaced by an atomic rename, so the
        // mapped inode never changes under us.
        if let Ok(r) = unsafe { Reader::open_mmap(dir.join("dbip-city.mmdb")) } {
            *self.city.write() = Some(r);
        }
        if let Ok(r) = Reader::open_readfile(dir.join("dbip-asn.mmdb")) {
            *self.asn.write() = Some(r);
        }
        self.cache.lock().clear();
    }

    pub fn loaded(&self) -> bool {
        self.city.read().is_some()
    }
}

fn age_days(p: &Path) -> Option<u64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(m.elapsed().ok()?.as_secs() / 86400)
}

async fn download(app: &App, kind: &str) -> anyhow::Result<()> {
    let dest = app.data_dir.join(format!("dbip-{kind}.mmdb"));
    let now = std::time::SystemTime::now();
    let secs = now.duration_since(std::time::UNIX_EPOCH)?.as_secs() as i64;
    // Try this month, then last month (new files appear during the month).
    let (y, m) = ym(secs);
    let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
    let mut last_err = None;
    for (yy, mm) in [(y, m), (py, pm)] {
        let url = format!("https://download.db-ip.com/free/dbip-{kind}-lite-{yy}-{mm:02}.mmdb.gz");
        match app.http_plain.get(&url).send().await.and_then(|r| r.error_for_status()) {
            Ok(resp) => {
                let gz = resp.bytes().await?;
                let tmp = dest.with_extension("tmp");
                {
                    let mut f = std::fs::File::create(&tmp)?;
                    std::io::copy(&mut flate2::read::GzDecoder::new(&gz[..]), &mut f)?;
                }
                drop(gz);
                // validate before swapping it in
                // SAFETY: tmp is ours and not modified while mapped.
                unsafe { Reader::open_mmap(&tmp)? };
                std::fs::rename(tmp, &dest)?;
                info!("downloaded {url}");
                return Ok(());
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.map(Into::into).unwrap_or_else(|| anyhow::anyhow!("download failed")))
}

/// Civil year/month from unix seconds (UTC).
fn ym(secs: i64) -> (i64, u32) {
    let days = secs.div_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m)
}

pub async fn run(app: Arc<App>) {
    app.geo.load(&app.data_dir);
    loop {
        let mut changed = false;
        let _ = std::fs::remove_file(app.data_dir.join("dbip-country.mmdb")); // replaced by city
        for kind in ["city", "asn"] {
            let p = app.data_dir.join(format!("dbip-{kind}.mmdb"));
            if age_days(&p).map(|d| d < 30).unwrap_or(false) {
                continue;
            }
            match download(&app, kind).await {
                Ok(()) => changed = true,
                Err(e) => warn!("geo database {kind}: {e:#}"),
            }
        }
        if changed {
            app.geo.load(&app.data_dir);
        }
        tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ym() {
        assert_eq!(super::ym(1793998932), (2026, 11));
        assert_eq!(super::ym(0), (1970, 1));
    }
}
