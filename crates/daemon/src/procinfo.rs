//! Process and socket inspection through libproc (the same API `lsof` uses).
//!
//! Running as root we see every process; as a normal user only our own.

use libproc::libproc::bsd_info::BSDInfo;
use libproc::libproc::file_info::{pidfdinfo, ListFDs, ProcFDType};
use libproc::libproc::net_info::{InSockInfo, SocketFDInfo, SocketInfoKind, TcpSIState};
use libproc::libproc::proc_pid::{listpidinfo, name, pidinfo, pidpath};
use libproc::processes::{pids_by_type, ProcFilter};
use parking_lot::Mutex;
use portward_core::model::ProcessInfo;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    pub fn as_str(self) -> &'static str {
        match self {
            Proto::Tcp => "tcp",
            Proto::Udp => "udp",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SockEntry {
    pub pid: i32,
    pub proto: Proto,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    /// TCP state name, `""` for UDP.
    pub state: &'static str,
}

impl SockEntry {
    pub fn is_listener(&self) -> bool {
        self.state == "LISTEN" || (self.proto == Proto::Udp && self.remote.port() == 0)
    }
}

fn addr_of(info: &InSockInfo, foreign: bool) -> SocketAddr {
    // SAFETY: the union variant is selected by insi_vflag (1 = IPv4, 2 = IPv6).
    let (a, port) = if foreign {
        (info.insi_faddr, info.insi_fport)
    } else {
        (info.insi_laddr, info.insi_lport)
    };
    let port = u16::from_be(port as u16);
    let ip = unsafe {
        if info.insi_vflag & 0x2 != 0 && info.insi_vflag & 0x1 == 0 {
            let v6 = Ipv6Addr::from(a.ina_6.s6_addr);
            match v6.to_ipv4_mapped() {
                Some(v4) => IpAddr::V4(v4),
                None => IpAddr::V6(v6),
            }
        } else {
            IpAddr::V4(Ipv4Addr::from(u32::from_be(a.ina_46.i46a_addr4.s_addr)))
        }
    };
    SocketAddr::new(ip, port)
}

fn tcp_state(s: i32) -> &'static str {
    match TcpSIState::from(s) {
        TcpSIState::Closed => "CLOSED",
        TcpSIState::Listen => "LISTEN",
        TcpSIState::SynSent => "SYN_SENT",
        TcpSIState::SynReceived => "SYN_RECEIVED",
        TcpSIState::Established => "ESTABLISHED",
        TcpSIState::CloseWait => "CLOSE_WAIT",
        TcpSIState::FinWait1 => "FIN_WAIT_1",
        TcpSIState::Closing => "CLOSING",
        TcpSIState::LastAck => "LAST_ACK",
        TcpSIState::FinWait2 => "FIN_WAIT_2",
        TcpSIState::TimeWait => "TIME_WAIT",
        _ => "UNKNOWN",
    }
}

/// Lists the internet sockets of one process.
pub fn sockets_of(pid: i32, out: &mut Vec<SockEntry>) {
    let Ok(info) = pidinfo::<BSDInfo>(pid, 0) else { return };
    let Ok(fds) = listpidinfo::<ListFDs>(pid, info.pbi_nfiles as usize) else { return };
    for fd in fds {
        if !matches!(ProcFDType::from(fd.proc_fdtype), ProcFDType::Socket) {
            continue;
        }
        let Ok(si) = pidfdinfo::<SocketFDInfo>(pid, fd.proc_fd) else { continue };
        let psi = &si.psi;
        if psi.soi_family != libc::AF_INET && psi.soi_family != libc::AF_INET6 {
            continue;
        }
        // SAFETY: soi_kind tells which union member the kernel filled in.
        let entry = unsafe {
            match SocketInfoKind::from(psi.soi_kind) {
                SocketInfoKind::Tcp => {
                    let t = psi.soi_proto.pri_tcp;
                    SockEntry {
                        pid,
                        proto: Proto::Tcp,
                        local: addr_of(&t.tcpsi_ini, false),
                        remote: addr_of(&t.tcpsi_ini, true),
                        state: tcp_state(t.tcpsi_state),
                    }
                }
                SocketInfoKind::In if psi.soi_protocol == libc::IPPROTO_UDP => {
                    let i = psi.soi_proto.pri_in;
                    SockEntry {
                        pid,
                        proto: Proto::Udp,
                        local: addr_of(&i, false),
                        remote: addr_of(&i, true),
                        state: "",
                    }
                }
                _ => continue,
            }
        };
        out.push(entry);
    }
}

/// Lists the internet sockets of all visible processes.
pub fn all_sockets() -> Vec<SockEntry> {
    let mut out = Vec::with_capacity(1024);
    for pid in pids_by_type(ProcFilter::All).unwrap_or_default() {
        if pid == 0 {
            continue;
        }
        sockets_of(pid as i32, &mut out);
    }
    out
}

/// Short-lived process info cache (pids get reused, so entries expire).
fn cache() -> &'static Mutex<HashMap<i32, (ProcessInfo, std::time::Instant)>> {
    static C: OnceLock<Mutex<HashMap<i32, (ProcessInfo, std::time::Instant)>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

pub fn process_info(pid: i32) -> ProcessInfo {
    let now = std::time::Instant::now();
    if let Some((p, t)) = cache().lock().get(&pid) {
        if now.duration_since(*t).as_secs() < 30 {
            return p.clone();
        }
    }
    let path = pidpath(pid).unwrap_or_default();
    let uid = pidinfo::<BSDInfo>(pid, 0).map(|i| i.pbi_uid).unwrap_or(0);
    let name = display_name(&path).unwrap_or_else(|| name(pid).unwrap_or_else(|_| format!("pid {pid}")));
    let app_path = group_path(&path);
    let info = ProcessInfo { pid, name, path, app_path, uid };
    let mut c = cache().lock();
    if c.len() > 4096 {
        c.retain(|_, (_, t)| now.duration_since(*t).as_secs() < 30);
    }
    c.insert(pid, (info.clone(), now));
    info
}

/// Friendly name: the outermost `.app` bundle name (so helpers are shown as
/// their app), otherwise the binary name.
pub fn display_name(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let bundle = path
        .split('/')
        .find(|c| c.ends_with(".app"))
        .map(|c| c.trim_end_matches(".app").to_string());
    bundle.or_else(|| path.rsplit('/').next().map(str::to_string))
}

/// Path of the outermost `.app` bundle containing `path`, or `path` itself.
pub fn group_path(path: &str) -> String {
    match path.find(".app/") {
        Some(i) => path[..i + 4].to_string(),
        None => path.to_string(),
    }
}

/// Finds the process owning a UDP or TCP socket bound to `local_port` on loopback.
pub fn find_pid_by_local_port(proto: Proto, local_port: u16) -> Option<i32> {
    let mut buf = Vec::new();
    for pid in pids_by_type(ProcFilter::All).unwrap_or_default() {
        if pid == 0 {
            continue;
        }
        buf.clear();
        sockets_of(pid as i32, &mut buf);
        if buf.iter().any(|s| s.proto == proto && s.local.port() == local_port) {
            return Some(pid as i32);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(
            display_name("/Applications/Google Chrome.app/Contents/Frameworks/x/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper").as_deref(),
            Some("Google Chrome")
        );
        assert_eq!(group_path("/Applications/Brave Browser.app/Contents/Frameworks/H.app/Contents/MacOS/H"), "/Applications/Brave Browser.app");
        assert_eq!(group_path("/usr/bin/curl"), "/usr/bin/curl");
        assert_eq!(display_name("/usr/bin/curl").as_deref(), Some("curl"));
    }

    #[test]
    fn sees_own_socket() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let _c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut v = vec![];
        sockets_of(std::process::id() as i32, &mut v);
        assert!(v.iter().any(|s| s.local.port() == port && s.state == "LISTEN"), "{v:?}");
        assert!(v.iter().any(|s| s.remote.port() == port && s.state == "ESTABLISHED"), "{v:?}");
        assert_eq!(find_pid_by_local_port(Proto::Tcp, port), Some(std::process::id() as i32));
    }
}
