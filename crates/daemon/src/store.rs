//! Persistent history (SQLite). Writes go through a channel to one writer
//! thread so the hot paths never block on disk.

use portward_core::model::{Connection, DnsEvent};
use rusqlite::{params, Connection as Db};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use tracing::warn;

enum Op {
    Dns(DnsEvent),
    Conn(Connection),
    End(Connection),
    Prune(u32),
}

pub struct Store {
    tx: Sender<Op>,
    path: PathBuf,
}

const SCHEMA: &str = "
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS dns (
  time INTEGER NOT NULL, domain TEXT NOT NULL, qtype TEXT, blocked INTEGER,
  reason TEXT, answers TEXT, app TEXT, app_path TEXT);
CREATE INDEX IF NOT EXISTS dns_time ON dns(time);
CREATE TABLE IF NOT EXISTS connections (
  id TEXT PRIMARY KEY, first_seen INTEGER, last_seen INTEGER, protocol TEXT,
  remote_ip TEXT, remote_port INTEGER, domain TEXT, scope TEXT, inbound INTEGER,
  blocked INTEGER, app TEXT, app_path TEXT, pid INTEGER);
CREATE INDEX IF NOT EXISTS conn_time ON connections(first_seen);
CREATE INDEX IF NOT EXISTS conn_app ON connections(app_path);
";

#[derive(Serialize)]
pub struct HistConn {
    pub first_seen: i64,
    pub last_seen: i64,
    pub protocol: String,
    pub remote_ip: String,
    pub remote_port: i64,
    pub domain: Option<String>,
    pub scope: String,
    pub inbound: bool,
    pub blocked: bool,
    pub app: String,
    pub app_path: String,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Db::open(path)?;
        db.execute_batch(SCHEMA)?;
        // Columns added in 0.2 (monitor mode); errors mean they already exist.
        for col in ["bytes_in INTEGER DEFAULT 0", "bytes_out INTEGER DEFAULT 0", "country TEXT", "org TEXT", "iface TEXT", "exposed_ip TEXT"] {
            let _ = db.execute(&format!("ALTER TABLE connections ADD COLUMN {col}"), []);
        }
        let (tx, rx) = channel::<Op>();
        std::thread::Builder::new().name("store".into()).spawn(move || {
            let mut batch = Vec::with_capacity(256);
            while let Ok(op) = rx.recv() {
                batch.push(op);
                while let Ok(op) = rx.try_recv() {
                    batch.push(op);
                    if batch.len() >= 1000 {
                        break;
                    }
                }
                if let Err(e) = write_batch(&db, &mut batch) {
                    warn!("history write: {e}");
                }
                batch.clear();
            }
        })?;
        Ok(Self { tx, path: path.to_path_buf() })
    }

    pub fn record_dns(&self, e: &DnsEvent) {
        let _ = self.tx.send(Op::Dns(e.clone()));
    }
    pub fn record_connection(&self, c: &Connection) {
        let _ = self.tx.send(Op::Conn(c.clone()));
    }
    pub fn end_connection(&self, c: &Connection) {
        let _ = self.tx.send(Op::End(c.clone()));
    }
    pub fn prune(&self, days: u32) {
        let _ = self.tx.send(Op::Prune(days));
    }

    /// Searches connection history. `q` matches domain, IP or app name.
    pub fn connections(&self, q: &str, app_path: &str, since: i64, limit: u32) -> anyhow::Result<Vec<HistConn>> {
        let db = Db::open(&self.path)?;
        let like = format!("%{q}%");
        let mut st = db.prepare(
            "SELECT first_seen,last_seen,protocol,remote_ip,remote_port,domain,scope,inbound,blocked,app,app_path
             FROM connections
             WHERE first_seen >= ?1 AND (?2 = '' OR domain LIKE ?3 OR remote_ip LIKE ?3 OR app LIKE ?3)
               AND (?4 = '' OR app_path = ?4)
             ORDER BY first_seen DESC LIMIT ?5",
        )?;
        let rows = st.query_map(params![since, q, like, app_path, limit], |r| {
            Ok(HistConn {
                first_seen: r.get(0)?,
                last_seen: r.get(1)?,
                protocol: r.get(2)?,
                remote_ip: r.get(3)?,
                remote_port: r.get(4)?,
                domain: r.get(5)?,
                scope: r.get(6)?,
                inbound: r.get::<_, i64>(7)? != 0,
                blocked: r.get::<_, i64>(8)? != 0,
                app: r.get(9)?,
                app_path: r.get(10)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Per-app totals over the history window.
    pub fn app_totals(&self, since: i64) -> anyhow::Result<Vec<(String, String, i64, i64, i64)>> {
        let db = Db::open(&self.path)?;
        let mut st = db.prepare(
            "SELECT app_path, app, COUNT(*), SUM(blocked), MAX(last_seen) FROM connections
             WHERE first_seen >= ?1 GROUP BY app_path ORDER BY COUNT(*) DESC",
        )?;
        let rows = st.query_map(params![since], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, Option<i64>>(3)?.unwrap_or(0), r.get(4)?))
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn app_domains(&self, app_path: &str, since: i64) -> anyhow::Result<Vec<(String, i64)>> {
        let db = Db::open(&self.path)?;
        let mut st = db.prepare(
            "SELECT COALESCE(domain, remote_ip), COUNT(*) FROM connections
             WHERE app_path = ?1 AND first_seen >= ?2 GROUP BY 1 ORDER BY 2 DESC LIMIT 200",
        )?;
        let rows = st.query_map(params![app_path, since], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }
}

fn write_batch(db: &Db, batch: &mut Vec<Op>) -> rusqlite::Result<()> {
    let tx = db.unchecked_transaction()?;
    for op in batch.drain(..) {
        match op {
            Op::Dns(e) => {
                tx.execute(
                    "INSERT INTO dns VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                    params![
                        e.time,
                        e.domain,
                        e.qtype,
                        e.blocked as i64,
                        e.reason,
                        e.answers.join(","),
                        e.process.as_ref().map(|p| p.name.clone()),
                        e.process.as_ref().map(|p| p.app_path.clone()),
                    ],
                )?;
            }
            Op::Conn(c) | Op::End(c) => {
                tx.execute(
                    "INSERT OR REPLACE INTO connections (id,first_seen,last_seen,protocol,remote_ip,remote_port,domain,scope,inbound,blocked,app,app_path,pid,bytes_in,bytes_out,country,org,iface,exposed_ip)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
                    params![
                        c.id,
                        c.first_seen,
                        c.last_seen,
                        c.protocol,
                        c.remote_ip,
                        c.remote_port,
                        c.domain,
                        c.scope,
                        c.inbound as i64,
                        c.blocked as i64,
                        c.process.name,
                        c.process.app_path,
                        c.process.pid,
                        c.bytes_in as i64,
                        c.bytes_out as i64,
                        c.geo.as_ref().and_then(|g| g.country.clone()),
                        c.geo.as_ref().and_then(|g| g.org.clone()),
                        c.interface,
                        c.exposed_ip,
                    ],
                )?;
            }
            Op::Prune(days) => {
                let cutoff = crate::state::now() - days as i64 * 86400;
                tx.execute("DELETE FROM dns WHERE time < ?1", params![cutoff])?;
                tx.execute("DELETE FROM connections WHERE first_seen < ?1", params![cutoff])?;
            }
        }
    }
    tx.commit()
}
