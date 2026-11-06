//! Upstream resolvers: DNS-over-HTTPS and plain UDP (with TCP fallback).

use anyhow::{bail, Context, Result};
use portward_core::config::{Upstream, UpstreamKind};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};

const TIMEOUT: Duration = Duration::from_secs(4);

/// Builds the HTTP client used for DoH. Every DoH host is pinned to its
/// bootstrap IPs, so resolving the resolver never goes through ourselves.
pub fn build_client(upstreams: &[Upstream]) -> reqwest::Client {
    let mut b = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(TIMEOUT)
        .pool_idle_timeout(Duration::from_secs(90))
        .user_agent(concat!("portward/", env!("CARGO_PKG_VERSION")));
    for u in upstreams.iter().filter(|u| u.kind == UpstreamKind::Doh) {
        let Ok(url) = reqwest::Url::parse(&u.address) else { continue };
        let Some(host) = url.host_str() else { continue };
        let port = url.port_or_known_default().unwrap_or(443);
        let addrs: Vec<SocketAddr> = u
            .bootstrap
            .iter()
            .filter_map(|ip| ip.parse().ok())
            .map(|ip| SocketAddr::new(ip, port))
            .collect();
        if !addrs.is_empty() {
            b = b.resolve_to_addrs(host, &addrs);
        }
    }
    b.build().expect("http client")
}

pub async fn query(client: &reqwest::Client, up: &Upstream, packet: &[u8]) -> Result<Vec<u8>> {
    match up.kind {
        UpstreamKind::Doh => doh(client, &up.address, packet).await,
        UpstreamKind::Udp => udp(&up.address, packet).await,
    }
}

async fn doh(client: &reqwest::Client, url: &str, packet: &[u8]) -> Result<Vec<u8>> {
    let resp = client
        .post(url)
        .header("content-type", "application/dns-message")
        .header("accept", "application/dns-message")
        .body(packet.to_vec())
        .send()
        .await
        .context("doh request")?;
    if !resp.status().is_success() {
        bail!("doh status {}", resp.status());
    }
    Ok(resp.bytes().await?.to_vec())
}

async fn udp(addr: &str, packet: &[u8]) -> Result<Vec<u8>> {
    let server: SocketAddr = addr.parse().context("upstream address")?;
    let bind: SocketAddr = if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse()?;
    let sock = UdpSocket::bind(bind).await?;
    sock.connect(server).await?;
    sock.send(packet).await?;
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(TIMEOUT, sock.recv(&mut buf)).await.context("udp timeout")??;
    buf.truncate(n);
    // TC bit set → retry over TCP.
    if n > 2 && buf[2] & 0x02 != 0 {
        return tcp(server, packet).await;
    }
    Ok(buf)
}

async fn tcp(server: SocketAddr, packet: &[u8]) -> Result<Vec<u8>> {
    let fut = async {
        let mut s = TcpStream::connect(server).await?;
        s.write_all(&(packet.len() as u16).to_be_bytes()).await?;
        s.write_all(packet).await?;
        let mut len = [0u8; 2];
        s.read_exact(&mut len).await?;
        let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
        s.read_exact(&mut buf).await?;
        anyhow::Ok(buf)
    };
    tokio::time::timeout(TIMEOUT, fut).await.context("tcp timeout")?
}
