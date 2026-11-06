//! Minimal, allocation-free packet parsing for passive monitoring.
//! Only headers are read; payloads are inspected for DNS answers and the TLS
//! server name (SNI) of new HTTPS connections, nothing else.

use crate::procinfo::Proto;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const DLT_NULL: i32 = 0;
pub const DLT_EN10MB: i32 = 1;
pub const DLT_RAW: i32 = 12;
pub const DLT_RAW2: i32 = 101;
pub const DLT_LOOP: i32 = 108;

#[derive(Debug, PartialEq)]
pub struct Pkt<'a> {
    pub proto: Proto,
    pub src: SocketAddr,
    pub dst: SocketAddr,
    /// IP packet length (header + payload), what the wire carried.
    pub len: u32,
    pub payload: &'a [u8],
    /// TCP flags (FIN=1 SYN=2 RST=4 PSH=8 ACK=16), 0 for UDP.
    pub flags: u8,
    /// IPv4 TTL / IPv6 hop limit.
    pub ttl: u8,
    /// The whole IP packet as captured (for the hex view).
    pub raw: &'a [u8],
}

pub fn flag_names(f: u8) -> String {
    let mut v = vec![];
    for (bit, n) in [(2, "SYN"), (16, "ACK"), (8, "PSH"), (1, "FIN"), (4, "RST")] {
        if f & bit != 0 {
            v.push(n);
        }
    }
    v.join(", ")
}

/// Strips the link layer and returns the IP packet.
pub fn link_to_ip(linktype: i32, data: &[u8]) -> Option<&[u8]> {
    match linktype {
        DLT_EN10MB => {
            let mut off = 12;
            let mut et = u16::from_be_bytes([*data.get(off)?, *data.get(off + 1)?]);
            // 802.1Q / QinQ VLAN tags.
            while et == 0x8100 || et == 0x88a8 {
                off += 4;
                et = u16::from_be_bytes([*data.get(off)?, *data.get(off + 1)?]);
            }
            match et {
                0x0800 | 0x86dd => data.get(off + 2..),
                _ => None,
            }
        }
        // 4-byte address family header (host order for NULL, network for LOOP);
        // we just look at the IP version nibble instead.
        DLT_NULL | DLT_LOOP => data.get(4..),
        DLT_RAW | DLT_RAW2 => Some(data),
        _ => None,
    }
}

pub fn parse_ip(ip: &[u8]) -> Option<Pkt<'_>> {
    let ver = ip.first()? >> 4;
    let (src, dst, proto, l4, len, ttl) = match ver {
        4 => {
            let ihl = ((ip[0] & 0x0f) as usize) * 4;
            if ip.len() < 20 || ihl < 20 {
                return None;
            }
            // Skip non-first fragments.
            let frag = u16::from_be_bytes([ip[6], ip[7]]) & 0x1fff;
            if frag != 0 {
                return None;
            }
            let total = u16::from_be_bytes([ip[2], ip[3]]) as usize;
            let src = IpAddr::V4(Ipv4Addr::new(ip[12], ip[13], ip[14], ip[15]));
            let dst = IpAddr::V4(Ipv4Addr::new(ip[16], ip[17], ip[18], ip[19]));
            let end = total.min(ip.len()).max(ihl);
            (src, dst, ip[9], ip.get(ihl..end)?, total as u32, ip[8])
        }
        6 => {
            if ip.len() < 40 {
                return None;
            }
            let plen = u16::from_be_bytes([ip[4], ip[5]]) as usize;
            let s: [u8; 16] = ip[8..24].try_into().ok()?;
            let d: [u8; 16] = ip[24..40].try_into().ok()?;
            // Extension headers are rare for TCP/UDP flows; only skip the
            // common hop-by-hop / routing / destination options.
            let mut next = ip[6];
            let mut off = 40;
            while matches!(next, 0 | 43 | 60) {
                next = *ip.get(off)?;
                off += (*ip.get(off + 1)? as usize + 1) * 8;
            }
            let end = (40 + plen).min(ip.len()).max(off);
            (IpAddr::V6(Ipv6Addr::from(s)), IpAddr::V6(Ipv6Addr::from(d)), next, ip.get(off..end)?, (40 + plen) as u32, ip[7])
        }
        _ => return None,
    };
    match proto {
        6 => {
            if l4.len() < 20 {
                return None;
            }
            let doff = ((l4[12] >> 4) as usize) * 4;
            Some(Pkt {
                proto: Proto::Tcp,
                src: SocketAddr::new(src, u16::from_be_bytes([l4[0], l4[1]])),
                dst: SocketAddr::new(dst, u16::from_be_bytes([l4[2], l4[3]])),
                len,
                payload: l4.get(doff..).unwrap_or(&[]),
                flags: l4[13],
                ttl,
                raw: ip,
            })
        }
        17 => {
            if l4.len() < 8 {
                return None;
            }
            Some(Pkt {
                proto: Proto::Udp,
                src: SocketAddr::new(src, u16::from_be_bytes([l4[0], l4[1]])),
                dst: SocketAddr::new(dst, u16::from_be_bytes([l4[2], l4[3]])),
                len,
                payload: &l4[8..],
                flags: 0,
                ttl,
                raw: ip,
            })
        }
        _ => None,
    }
}

/// Extracts the server name from a TLS ClientHello (first TCP segment).
pub fn parse_sni(p: &[u8]) -> Option<String> {
    // TLS record: type 22 (handshake), version 3.x
    if p.len() < 43 || p[0] != 0x16 || p[1] != 0x03 || p[5] != 0x01 {
        return None;
    }
    let mut i = 5 + 4; // record header + handshake header
    i += 2 + 32; // client version + random
    let sid = *p.get(i)? as usize;
    i += 1 + sid;
    let cs = u16::from_be_bytes([*p.get(i)?, *p.get(i + 1)?]) as usize;
    i += 2 + cs;
    let cm = *p.get(i)? as usize;
    i += 1 + cm;
    let ext_len = u16::from_be_bytes([*p.get(i)?, *p.get(i + 1)?]) as usize;
    i += 2;
    let end = (i + ext_len).min(p.len());
    while i + 4 <= end {
        let typ = u16::from_be_bytes([p[i], p[i + 1]]);
        let len = u16::from_be_bytes([p[i + 2], p[i + 3]]) as usize;
        i += 4;
        if typ == 0 {
            // server_name list: len(2) type(1) name_len(2) name
            let name_len = u16::from_be_bytes([*p.get(i + 3)?, *p.get(i + 4)?]) as usize;
            let name = p.get(i + 5..i + 5 + name_len)?;
            let s = std::str::from_utf8(name).ok()?;
            if s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) && s.contains('.') {
                return Some(s.to_ascii_lowercase());
            }
            return None;
        }
        i += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipv4_udp(src: [u8; 4], sp: u16, dst: [u8; 4], dp: u16, payload: &[u8]) -> Vec<u8> {
        let total = 20 + 8 + payload.len();
        let mut v = vec![0x45, 0, (total >> 8) as u8, total as u8, 0, 0, 0x40, 0, 64, 17, 0, 0];
        v.extend(src);
        v.extend(dst);
        v.extend(sp.to_be_bytes());
        v.extend(dp.to_be_bytes());
        v.extend(((8 + payload.len()) as u16).to_be_bytes());
        v.extend([0, 0]);
        v.extend(payload);
        v
    }

    #[test]
    fn ethernet_udp() {
        let ip = ipv4_udp([192, 168, 1, 19], 5000, [8, 8, 8, 8], 53, b"hello");
        let mut frame = vec![0u8; 12];
        frame.extend([0x08, 0x00]);
        frame.extend(&ip);
        let p = parse_ip(link_to_ip(DLT_EN10MB, &frame).unwrap()).unwrap();
        assert_eq!(p.proto, Proto::Udp);
        assert_eq!(p.src, "192.168.1.19:5000".parse().unwrap());
        assert_eq!(p.dst, "8.8.8.8:53".parse().unwrap());
        assert_eq!(p.payload, b"hello");
        assert_eq!(p.len, 33);
    }

    #[test]
    fn utun_ipv6_tcp() {
        let mut ip = vec![0x60, 0, 0, 0, 0, 20, 6, 64];
        ip.extend("2001:db8::1".parse::<Ipv6Addr>().unwrap().octets());
        ip.extend("2606:4700::1111".parse::<Ipv6Addr>().unwrap().octets());
        let mut tcp = vec![0u8; 20];
        tcp[0..2].copy_from_slice(&50000u16.to_be_bytes());
        tcp[2..4].copy_from_slice(&443u16.to_be_bytes());
        tcp[12] = 5 << 4;
        ip.extend(tcp);
        let mut frame = vec![30, 0, 0, 0];
        frame.extend(ip);
        let p = parse_ip(link_to_ip(DLT_NULL, &frame).unwrap()).unwrap();
        assert_eq!(p.proto, Proto::Tcp);
        assert_eq!(p.dst.port(), 443);
        assert_eq!(p.len, 60);
        assert!(p.payload.is_empty());
    }

    #[test]
    fn sni() {
        // Minimal ClientHello with an SNI extension for "discord.com".
        let name = b"discord.com";
        let mut ext = vec![0, 0];
        let list_len = 3 + name.len();
        ext.extend(((list_len + 2) as u16).to_be_bytes());
        ext.extend((list_len as u16).to_be_bytes());
        ext.push(0);
        ext.extend((name.len() as u16).to_be_bytes());
        ext.extend(name);
        let mut body = vec![0x03, 0x03];
        body.extend([0u8; 32]);
        body.push(0); // session id
        body.extend([0, 2, 0x13, 0x01]); // cipher suites
        body.extend([1, 0]); // compression
        body.extend((ext.len() as u16).to_be_bytes());
        body.extend(&ext);
        let mut hs = vec![1, 0, (body.len() >> 8) as u8, body.len() as u8];
        hs.extend(&body);
        let mut rec = vec![0x16, 0x03, 0x01];
        rec.extend((hs.len() as u16).to_be_bytes());
        rec.extend(&hs);
        assert_eq!(parse_sni(&rec).as_deref(), Some("discord.com"));
        assert_eq!(parse_sni(b"GET / HTTP/1.1\r\n"), None);
    }
}
