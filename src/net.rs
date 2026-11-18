//! Dialling a host so the network sees neither its name nor what is said: no plain DNS lookup, no
//! name in the TLS handshake. Wireshark on the Pi's network sees TLS to a Cloudflare address, and nothing
//! that says which site.
//!
//! ```text
//!   resolve ─► DNS over HTTPS to 1.1.1.1 (dialled by address: no name in that handshake either)
//!                ├─ A record       where to connect
//!                └─ HTTPS record   ech=…: the site's Encrypted Client Hello keys
//!   sealed  ─► TCP to an A address ─► TLS 1.3, the ClientHello encrypted to those keys
//!                on the wire: SNI cloudflare-ech.com · inside, encrypted: dbrs.neokai.org
//! ```
//!
//! A host that publishes no ECH keys, or a server that refuses them, is not dialled at all: falling
//! back to a plain handshake would put the name on the wire.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use http_body_util::{BodyExt as _, Empty, Limited};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use rustls::client::{EchConfig, EchMode, EchStatus};
use rustls::pki_types::{EchConfigListBytes, ServerName};
use rustls::version::TLS13;
use rustls::{ClientConfig, ConfigBuilder, WantsVerifier};
use rustls_platform_verifier::BuilderVerifierExt as _;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tracing::debug;

/// Cloudflare's resolver, by address. Its certificate names the address, so it is checked like any site's.
const RESOLVER: IpAddr = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
const HTTPS_PORT: u16 = 443;
const DOH_PATH: &str = "/dns-query";
const DOH_JSON: &str = "application/dns-json";
const ANSWER_MAX: usize = 64 * 1024;
const STEP_TIMEOUT: Duration = Duration::from_secs(10);
// DNS record types (RFC 1035, RFC 9460) and the answer status that means found.
const TYPE_A: u16 = 1;
const TYPE_HTTPS: u16 = 65;
const NO_ERROR: u16 = 0;
const ECH_KEY: &str = "ech=";
const HTTP1: &[u8] = b"http/1.1";

#[derive(Deserialize)]
struct Answer {
    #[serde(rename = "Status")]
    status: u16,
    #[serde(rename = "Answer", default)]
    records: Vec<Record>,
}

#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type")]
    kind: u16,
    data: String,
}

/// Where a host is, and the keys its ClientHello is encrypted to.
pub(crate) struct Sealed {
    addresses: Vec<IpAddr>,
    ech: Vec<u8>,
}

// TLS 1.3 only, ring for the ciphers, the system's roots for trust.
fn tls13() -> Result<ConfigBuilder<ClientConfig, WantsVerifier>> {
    Ok(ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider())).with_protocol_versions(&[&TLS13])?)
}

async fn timed<T>(what: &str, step: impl Future<Output = T>) -> Result<T> {
    tokio::time::timeout(STEP_TIMEOUT, step).await.with_context(|| format!("{what}: no answer in {}s", STEP_TIMEOUT.as_secs()))
}

/// The ECH keys in an HTTPS record's text: `1 . alpn=h3,h2 ech=AEX+… ipv4hint=…`.
fn ech_of(data: &str) -> Option<Vec<u8>> {
    let encoded = data.split_whitespace().find_map(|part| part.strip_prefix(ECH_KEY))?;
    BASE64.decode(encoded.trim_matches('"')).ok()
}

// One DNS question, asked over HTTPS.
async fn ask(name: &str, kind: u16) -> Result<Vec<Record>> {
    let mut config = tls13()?.with_platform_verifier()?.with_no_client_auth();
    config.alpn_protocols = vec![HTTP1.to_vec()];
    let tcp = timed("reaching the resolver", TcpStream::connect(SocketAddr::new(RESOLVER, HTTPS_PORT))).await??;
    let tls = timed("the resolver's handshake", TlsConnector::from(Arc::new(config)).connect(ServerName::IpAddress(RESOLVER.into()), tcp)).await??;

    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(tls)).await.context("talking HTTP to the resolver")?;
    tokio::spawn(connection);
    let request = hyper::Request::get(format!("{DOH_PATH}?name={name}&type={kind}"))
        .header(hyper::header::HOST, RESOLVER.to_string())
        .header(hyper::header::ACCEPT, DOH_JSON)
        .body(Empty::<Bytes>::new())?;
    let response = timed("the resolver's answer", sender.send_request(request)).await??;
    if !response.status().is_success() {
        bail!("the resolver answered {}", response.status());
    }
    let body = Limited::new(response.into_body(), ANSWER_MAX).collect().await.map_err(|source| anyhow::anyhow!("reading the resolver's answer: {source}"))?;
    let answer: Answer = serde_json::from_slice(&body.to_bytes()).context("the resolver's answer is not DNS JSON")?;
    if answer.status != NO_ERROR {
        bail!("the resolver found no {name} (status {})", answer.status);
    }
    Ok(answer.records.into_iter().filter(|r| r.kind == kind).collect())
}

/// Where `host` is and its ECH keys, looked up without a plain DNS query.
pub(crate) async fn resolve(host: &str) -> Result<Sealed> {
    let (a, https) = tokio::join!(ask(host, TYPE_A), ask(host, TYPE_HTTPS));
    let addresses: Vec<IpAddr> = a?.iter().filter_map(|r| r.data.parse().ok()).collect();
    if addresses.is_empty() {
        bail!("{host} has no address");
    }
    let Some(ech) = https?.iter().find_map(|r| ech_of(&r.data)) else {
        bail!("{host} publishes no Encrypted Client Hello keys; not dialling it, its name would show on the wire");
    };
    Ok(Sealed { addresses, ech })
}

/// A TLS 1.3 connection to `host`, its name sent only inside the encrypted ClientHello.
pub(crate) async fn sealed(host: &str, port: u16, alpn: &[u8]) -> Result<TlsStream<TcpStream>> {
    let found = resolve(host).await?;
    let ech = EchConfig::new(EchConfigListBytes::from(found.ech.as_slice()), rustls::crypto::aws_lc_rs::hpke::ALL_SUPPORTED_SUITES)
        .context("the ECH keys are not usable")?;
    let mut config = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_ech(EchMode::Enable(ech))?
        .with_platform_verifier()?
        .with_no_client_auth();
    config.alpn_protocols = vec![alpn.to_vec()];
    let connector = TlsConnector::from(Arc::new(config));
    let name = ServerName::try_from(host.to_owned()).with_context(|| format!("{host} is not a host name"))?;

    let mut last = None;
    for address in found.addresses {
        let attempt = async {
            let tcp = timed("reaching the host", TcpStream::connect(SocketAddr::new(address, port))).await??;
            let tls = timed("the handshake", connector.connect(name.clone(), tcp)).await??;
            // rustls fails a handshake the server refused ECH on; this is the check that it was used.
            if tls.get_ref().1.ech_status() != EchStatus::Accepted {
                bail!("the server did not take the encrypted ClientHello");
            }
            anyhow::Ok(tls)
        };
        match attempt.await {
            Ok(tls) => {
                debug!(%address, "sealed connection up");
                return Ok(tls);
            }
            Err(source) => last = Some(source.context(format!("dialling {address}"))),
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("{host}: nothing to dial")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ech_keys_are_read_from_an_https_record() {
        let record = "1 . alpn=h3,h2 ipv4hint=104.21.23.112 ech=AEX+DQBBKgAgACCj6pofQwdbpG8pJKH7/ZpLCR+ah9aoaEVPjXSNX3tgdwAEAAEAAQASY2xvdWRmbGFyZS1lY2guY29tAAA= ipv6hint=2606:4700::1";
        let keys = ech_of(record).unwrap();
        assert_eq!(&keys[..2], [0x00, 0x45]);
        assert!(EchConfig::new(EchConfigListBytes::from(keys.as_slice()), rustls::crypto::aws_lc_rs::hpke::ALL_SUPPORTED_SUITES).is_ok());
        assert_eq!(ech_of("1 . alpn=h3,h2"), None);
        assert_eq!(ech_of("1 . ech=%%%"), None);
    }

    /// Dials the live relay host over the network: `cargo test -- --ignored sealed`.
    #[tokio::test]
    #[ignore = "reaches 1.1.1.1 and dbrs.neokai.org"]
    async fn the_relay_host_is_dialled_sealed() {
        let tls = sealed("dbrs.neokai.org", HTTPS_PORT, HTTP1).await.unwrap();
        assert_eq!(tls.get_ref().1.ech_status(), EchStatus::Accepted);
    }
}
