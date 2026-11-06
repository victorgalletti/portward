//! Passive traffic observation with libpcap (needs root for /dev/bpf).
//!
//! Nothing is modified or blocked here. From each packet we only take:
//! * which local/remote address pair it belongs to and its size (bytes per connection),
//! * DNS answers (to name the servers you talk to),
//! * the TLS server name of new HTTPS connections (works even when an app uses
//!   its own encrypted DNS).

use crate::packet::{link_to_ip, parse_ip, parse_sni};
use crate::procinfo::{self, Proto};
use crate::state::{now, App, Flow, FlowKey, PacketRec};
use hickory_proto::op::{Message, MessageType};
use hickory_proto::rr::RData;
use portward_core::config::Mode;
use portward_core::model::DnsEvent;
use portward_core::normalize_domain;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

const FLUSH: Duration = Duration::from_millis(700);

pub async fn run(app: Arc<App>) {
    if !app.root {
        app.warnings.write().push("Sem root não dá para observar o tráfego: bytes por conexão e nomes de servidores ficam incompletos.".into());
        return;
    }
    let mut running: HashMap<String, Arc<AtomicBool>> = HashMap::new();
    loop {
        crate::netif::refresh_local(&app);
        let ifs: HashSet<String> = app.local_ips.read().values().cloned().collect();
        // Stop captures on interfaces that went away.
        running.retain(|name, stop| {
            let keep = ifs.contains(name) && !stop.load(Ordering::Relaxed);
            if !keep {
                stop.store(true, Ordering::Relaxed);
            }
            keep
        });
        for name in ifs {
            if running.contains_key(&name) {
                continue;
            }
            let stop = Arc::new(AtomicBool::new(false));
            running.insert(name.clone(), stop.clone());
            let a = app.clone();
            std::thread::Builder::new()
                .name(format!("cap-{name}"))
                .spawn(move || {
                    if let Err(e) = capture_iface(&a, &name, &stop) {
                        warn!("capture on {name} stopped: {e:#}");
                    }
                    // Marked stopped so the manager can retry later.
                    stop.store(true, Ordering::Relaxed);
                })
                .ok();
        }
        *app.capture_ifaces.write() = running.keys().cloned().collect();
        tokio::time::sleep(Duration::from_secs(15)).await;
    }
}

#[derive(Default)]
struct Local {
    flows: HashMap<FlowKey, Flow>,
    names: Vec<(IpAddr, String)>,
    events: Vec<DnsEvent>,
    packets: Vec<PacketRec>,
}

const MAX_PACKETS: usize = 20_000;
const HEX_BYTES: usize = 128;

fn hex(b: &[u8]) -> String {
    b.iter().take(HEX_BYTES).map(|x| format!("{x:02x}")).collect()
}

/// Human summary of a packet, like Wireshark's "Info" column.
fn info_of(pkt: &crate::packet::Pkt<'_>, local_port: u16, remote_port: u16) -> String {
    use crate::packet::flag_names;
    let p = pkt.payload;
    match pkt.proto {
        Proto::Tcp => {
            let ports = format!("{} → {}", pkt.src.port(), pkt.dst.port());
            let fl = flag_names(pkt.flags);
            let what = if p.len() >= 6 && p[0] == 0x16 && p[1] == 0x03 {
                match p[5] {
                    1 => match parse_sni(p) {
                        Some(s) => format!("TLS ClientHello · SNI {s}"),
                        None => "TLS ClientHello".into(),
                    },
                    2 => "TLS ServerHello".into(),
                    _ => "TLS handshake".into(),
                }
            } else if p.len() >= 3 && p[0] == 0x17 && p[1] == 0x03 {
                format!("TLS dados criptografados ({} bytes)", p.len())
            } else if p.len() >= 3 && p[0] == 0x15 && p[1] == 0x03 {
                "TLS alerta".into()
            } else if p.starts_with(b"GET ") || p.starts_with(b"POST ") || p.starts_with(b"HTTP/") {
                let line = p.split(|&b| b == b'\r').next().unwrap_or(&[]);
                format!("HTTP {}", String::from_utf8_lossy(&line[..line.len().min(80)]))
            } else if !p.is_empty() {
                format!("{} bytes de dados", p.len())
            } else {
                String::new()
            };
            [ports, if fl.is_empty() { String::new() } else { format!("[{fl}]") }, what]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        }
        Proto::Udp => {
            let port = if remote_port == 53 || local_port == 53 { 53 } else { remote_port.min(local_port.max(remote_port)) };
            if remote_port == 53 || local_port == 53 {
                match Message::from_vec(p) {
                    Ok(m) => {
                        let q = m.queries.first().map(|q| format!("{} {}", q.query_type(), normalize_domain(&q.name().to_ascii()))).unwrap_or_default();
                        if m.metadata.message_type == MessageType::Response {
                            let first = m.answers.iter().find_map(|r| match &r.data {
                                RData::A(a) => Some(a.0.to_string()),
                                RData::AAAA(a) => Some(a.0.to_string()),
                                RData::CNAME(c) => Some(format!("CNAME {}", normalize_domain(&c.0.to_ascii()))),
                                _ => None,
                            });
                            format!("DNS resposta {q}{}", first.map(|f| format!(" → {f}")).unwrap_or_default())
                        } else {
                            format!("DNS consulta {q}")
                        }
                    }
                    Err(_) => "DNS (malformado)".into(),
                }
            } else if remote_port == 443 || local_port == 443 {
                match p.first() {
                    Some(b) if b & 0x80 != 0 => match (b >> 4) & 0x3 {
                        0 => "QUIC Initial (início de conexão)".into(),
                        2 => "QUIC Handshake".into(),
                        _ => "QUIC (cabeçalho longo)".into(),
                    },
                    Some(_) => format!("QUIC dados criptografados ({} bytes)", p.len()),
                    None => "QUIC".into(),
                }
            } else if remote_port == 123 {
                "NTP (hora)".into()
            } else {
                let _ = port;
                format!("UDP {} → {} · {} bytes", pkt.src.port(), pkt.dst.port(), p.len())
            }
        }
    }
}

fn capture_iface(app: &App, name: &str, stop: &AtomicBool) -> anyhow::Result<()> {
    let mut cap = pcap::Capture::from_device(name)?
        .snaplen(2048)
        .promisc(false)
        .immediate_mode(true)
        .timeout(500)
        .open()?;
    let lt = cap.get_datalink().0;
    info!("observing traffic on {name} (linktype {lt})");
    let mut local = Local::default();
    let mut pending: HashMap<(u16, u16), (Instant, IpAddr)> = HashMap::new();
    let mut last_flush = Instant::now();
    let mut locals = app.local_ips.read().keys().copied().collect::<HashSet<IpAddr>>();

    while !stop.load(Ordering::Relaxed) {
        match cap.next_packet() {
            Ok(p) => {
                if let Some(pkt) = link_to_ip(lt, p.data).and_then(parse_ip) {
                    handle(app, &locals, &mut local, &mut pending, pkt);
                }
            }
            Err(pcap::Error::TimeoutExpired) => {}
            Err(e) => return Err(e.into()),
        }
        if last_flush.elapsed() >= FLUSH {
            flush(app, &mut local, name);
            locals = app.local_ips.read().keys().copied().collect();
            let cutoff = Instant::now() - Duration::from_secs(10);
            pending.retain(|_, (t, _)| *t > cutoff);
            last_flush = Instant::now();
        }
    }
    flush(app, &mut local, name);
    Ok(())
}

fn handle(
    app: &App,
    locals: &HashSet<IpAddr>,
    local: &mut Local,
    pending: &mut HashMap<(u16, u16), (Instant, IpAddr)>,
    pkt: crate::packet::Pkt<'_>,
) {
    let outbound = if locals.contains(&pkt.src.ip()) {
        true
    } else if locals.contains(&pkt.dst.ip()) {
        false
    } else {
        return;
    };
    let (l, r) = if outbound { (pkt.src, pkt.dst) } else { (pkt.dst, pkt.src) };
    if local.packets.len() < 4000 {
        local.packets.push(PacketRec {
            id: 0,
            t: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
            iface: String::new(),
            outbound,
            proto: pkt.proto.as_str(),
            local: l.to_string(),
            remote: r.to_string(),
            len: pkt.len,
            ttl: pkt.ttl,
            flags: crate::packet::flag_names(pkt.flags),
            info: info_of(&pkt, l.port(), r.port()),
            hex: hex(pkt.raw),
        });
    }
    let f = local.flows.entry((pkt.proto, l, r)).or_default();
    if outbound {
        f.bytes_out += pkt.len as u64;
    } else {
        f.bytes_in += pkt.len as u64;
    }
    f.last = now();

    match (pkt.proto, outbound) {
        (Proto::Tcp, true) if f.sni.is_none() && pkt.payload.first() == Some(&0x16) => {
            if let Some(sni) = parse_sni(pkt.payload) {
                local.names.push((r.ip(), sni.clone()));
                f.sni = Some(sni);
            }
        }
        (Proto::Udp, true) if r.port() == 53 && pkt.payload.len() >= 12 => {
            let id = u16::from_be_bytes([pkt.payload[0], pkt.payload[1]]);
            if pending.len() < 10_000 {
                pending.insert((l.port(), id), (Instant::now(), r.ip()));
            }
        }
        (Proto::Udp, false) if r.port() == 53 => dns_answer(app, local, pending, l.port(), r.ip(), pkt.payload),
        _ => {}
    }
}

fn dns_answer(app: &App, local: &mut Local, pending: &mut HashMap<(u16, u16), (Instant, IpAddr)>, lport: u16, server: IpAddr, payload: &[u8]) {
    let Ok(m) = Message::from_vec(payload) else { return };
    if m.metadata.message_type != MessageType::Response || m.queries.is_empty() {
        return;
    }
    let q = &m.queries[0];
    let domain = normalize_domain(&q.name().to_ascii());
    let mut answers = vec![];
    for r in &m.answers {
        match &r.data {
            RData::A(a) => {
                local.names.push((a.0.into(), domain.clone()));
                answers.push(a.0.to_string());
            }
            RData::AAAA(a) => {
                local.names.push((a.0.into(), domain.clone()));
                answers.push(a.0.to_string());
            }
            RData::CNAME(c) => answers.push(format!("CNAME {}", normalize_domain(&c.0.to_ascii()))),
            _ => {}
        }
    }
    if app.cfg.read().mode != Mode::Monitor {
        return; // in filter mode our own server records the queries
    }
    let latency = pending
        .remove(&(lport, m.metadata.id))
        .map(|(t, _)| t.elapsed().as_millis() as u32)
        .unwrap_or(0);
    let process = app
        .port_owner
        .read()
        .get(&lport)
        .map(|pid| procinfo::process_info(*pid))
        .filter(|p| p.name != "mDNSResponder");
    local.events.push(DnsEvent {
        time: now(),
        domain,
        qtype: format!("{}", q.query_type()),
        blocked: false,
        reason: if m.metadata.response_code == hickory_proto::op::ResponseCode::NoError {
            String::new()
        } else {
            format!("{}", m.metadata.response_code)
        },
        answers,
        cached: false,
        upstream: Some(server.to_string()),
        latency_ms: latency,
        process,
    });
}

fn flush(app: &App, local: &mut Local, iface: &str) {
    if !local.packets.is_empty() {
        let mut ring = app.packets.lock();
        for mut p in local.packets.drain(..) {
            p.id = app.packet_seq.fetch_add(1, Ordering::Relaxed) + 1;
            p.iface = iface.to_string();
            ring.push_back(p);
        }
        while ring.len() > MAX_PACKETS {
            ring.pop_front();
        }
    }
    if !local.flows.is_empty() {
        let mut flows = app.flows.lock();
        for (k, f) in local.flows.drain() {
            let e = flows.entry(k).or_default();
            e.bytes_in += f.bytes_in;
            e.bytes_out += f.bytes_out;
            e.last = e.last.max(f.last);
            if e.sni.is_none() {
                e.sni = f.sni;
            }
        }
    }
    if !local.names.is_empty() {
        let t = now();
        let mut map = app.ip_domains.lock();
        for (ip, d) in local.names.drain(..) {
            map.insert(ip, (d, t));
        }
    }
    for ev in local.events.drain(..) {
        {
            let mut c = app.counters.lock();
            c.dns_queries += 1;
        }
        app.count_minute(false);
        app.push_event(ev);
    }
}
