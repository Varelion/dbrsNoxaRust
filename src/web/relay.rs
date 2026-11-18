//! The bot's end of the neoKai relay (neoKai/relay-dbrs): one outbound WebSocket to the Worker, so the
//! admin pages are public at dbrs.neokai.org without the Pi opening a port.
//!
//! ```text
//!   connect ─► hello{secret} ─► welcome ─► req ─► the admin routes ─► res
//!      ▲                                                               │
//!      └──────── closed: redial after 1 s, 2 s, … 30 s ◄──────────────┘
//!
//!   relay ─► bot  { t:"req", id, method, path, query, type, body, body64? }   body64: a binary body, base64
//!   bot ─► relay  { t:"res", id, status, headers, body, body64? }
//! ```
//!
//! The socket is TLS 1.3 with Encrypted Client Hello, its address looked up over DNS over HTTPS
//! (`net::sealed`): the network sees TLS to Cloudflare, not which site, nor the secret, nor the pages.
//!
//! Logs (never the secret, never a token): connected and after how long down, every close with its
//! code, a socket dropped for not answering pings, and each request that failed or was slow.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, header};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use futures_util::{SinkExt as _, StreamExt as _};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_websockets::{ClientBuilder, Limits, Message};
use axum::http::Uri;
use tower::ServiceExt as _;
use tracing::{error, info, warn};

const VERSION: &str = "noxadbrs";
const HELLO: &str = "hello";
const WELCOME: &str = "welcome";
const REQUEST: &str = "req";
const RESPONSE: &str = "res";
// Answered by the relay's runtime without waking it: keeps the path open.
const PING: &str = "ping";
const PONG: &str = "pong";
const PING_EVERY: Duration = Duration::from_secs(30);
// No pong for this long: the socket is half open; drop it and redial.
const DEAD_AFTER: Duration = Duration::from_secs(90);
const FIRST_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(30);
// A 10 MiB upload is about 13.4 MiB as base64; anything larger is refused before it is read.
const FRAME_MAX: usize = 16 * 1024 * 1024;
// What a page may answer with; the pages are a few dozen KiB.
const ANSWER_MAX: usize = 4 * 1024 * 1024;
const SLOW: Duration = Duration::from_secs(5);
const STATUS_ERROR: u16 = 500;
const CLOSE_REPLACED: u16 = 4000;
const CLOSE_UNAUTHORIZED: u16 = 4401;
const TOKEN_HEX: usize = 32;
const SECURE_SCHEME: &str = "wss";
const HTTPS_PORT: u16 = 443;
const HTTP1: &[u8] = b"http/1.1";

#[derive(Serialize)]
struct Hello<'a> {
    t: &'a str,
    secret: &'a str,
    version: &'a str,
}

/// A frame from the relay: only `t` matters until it is a request.
#[derive(Deserialize)]
struct Incoming {
    t: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    query: String,
    #[serde(default, rename = "type")]
    content_type: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    body64: Option<String>,
}

#[derive(Serialize)]
struct Outgoing {
    t: &'static str,
    id: String,
    status: u16,
    headers: HashMap<String, String>,
    body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    body64: Option<String>,
}

/// A path fit for logs: a link token (32 hex) becomes <token>.
fn redacted(path: &str) -> String {
    path.split('/')
        .map(|part| if part.len() == TOKEN_HEX && part.bytes().all(|b| b.is_ascii_hexdigit()) { "<token>" } else { part })
        .collect::<Vec<_>>()
        .join("/")
}

// Bodies that travel back as text; everything else (a picture, a font) as base64.
fn is_text(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    lower.starts_with("text/") || lower.starts_with("application/json")
}

fn request_of(frame: &Incoming) -> Result<Request<Body>> {
    let body = match frame.body64.as_deref() {
        Some(encoded) => BASE64.decode(encoded).context("a request body that is not base64")?,
        None => frame.body.clone().into_bytes(),
    };
    let mut request = Request::builder().method(frame.method.as_str()).uri(format!("{}{}", frame.path, frame.query));
    if !frame.content_type.is_empty() {
        request = request.header(header::CONTENT_TYPE, frame.content_type.as_str());
    }
    request.header(header::CONTENT_LENGTH, body.len()).body(Body::from(body)).context("a request the routes cannot take")
}

async fn frame_of(id: String, response: Response<Body>) -> Result<Outgoing> {
    let status = response.status().as_u16();
    let headers: HashMap<String, String> =
        response.headers().iter().filter_map(|(name, value)| Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))).collect();
    let content_type = headers.get(header::CONTENT_TYPE.as_str()).cloned().unwrap_or_default();
    let bytes = to_bytes(response.into_body(), ANSWER_MAX).await.context("reading the page")?;
    if is_text(&content_type) || bytes.is_empty() {
        let body = String::from_utf8(bytes.to_vec()).context("a text page that is not UTF-8")?;
        return Ok(Outgoing { t: RESPONSE, id, status, headers, body, body64: None });
    }
    Ok(Outgoing { t: RESPONSE, id, status, headers, body: String::new(), body64: Some(BASE64.encode(&bytes)) })
}

/// One request frame through the routes, as a response frame. A request that cannot be read is a 500.
async fn answer(routes: Router, frame: Incoming) -> Outgoing {
    let id = frame.id.clone();
    let failed = |id: String| Outgoing { t: RESPONSE, id, status: STATUS_ERROR, headers: HashMap::new(), body: String::new(), body64: None };
    let request = match request_of(&frame) {
        Ok(request) => request,
        Err(source) => {
            warn!(?source, path = %redacted(&frame.path), "a relay request could not be read");
            return failed(id);
        }
    };
    let response = match routes.oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    match frame_of(id.clone(), response).await {
        Ok(out) => out,
        Err(source) => {
            warn!(?source, path = %redacted(&frame.path), "a page could not be sent back through the relay");
            failed(id)
        }
    }
}

/// Keeps one socket to the relay open, for as long as the bot runs.
pub(crate) async fn run(url: String, secret: String, routes: Router) {
    let mut delay = FIRST_DELAY;
    let mut down_since = Instant::now();
    loop {
        match session(&url, &secret, &routes, &mut down_since).await {
            Ok(Ended::Welcomed) => delay = FIRST_DELAY,
            Ok(Ended::Refused) => {
                error!("the relay refused RELAY_SECRET; check it matches the Worker's secret");
                delay = MAX_DELAY;
            }
            Ok(Ended::NeverWelcomed) => {}
            Err(source) => warn!(?source, "relay connection failed"),
        }
        warn!(redial_in_s = delay.as_secs(), "relay down; redialling");
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_DELAY);
    }
}

/// How a socket ended, for the next wait.
enum Ended {
    Welcomed,
    NeverWelcomed,
    Refused,
}

async fn session(url: &str, secret: &str, routes: &Router, down_since: &mut Instant) -> Result<Ended> {
    let uri: Uri = url.parse().with_context(|| format!("RELAY_URL {url:?} is not a URL"))?;
    let (Some(SECURE_SCHEME), Some(host)) = (uri.scheme_str(), uri.host()) else {
        bail!("RELAY_URL must be wss://host/…: the secret never travels unencrypted");
    };
    let tls = crate::net::sealed(host, uri.port_u16().unwrap_or(HTTPS_PORT), HTTP1).await.context("dialling the relay sealed")?;
    let limits = Limits::default().max_payload_len(Some(FRAME_MAX));
    let (mut socket, _) = ClientBuilder::from_uri(uri).limits(limits).connect_on(tls).await.context("opening the relay socket")?;
    let hello = serde_json::to_string(&Hello { t: HELLO, secret, version: VERSION })?;
    socket.send(Message::text(hello)).await.context("sending hello")?;

    // Answers come back from their own tasks: a slow page never holds up the next request.
    let (answers, mut answered) = mpsc::unbounded_channel::<(String, Instant, Outgoing)>();
    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await;
    let mut last_pong = Instant::now();
    let mut up_since: Option<Instant> = None;

    loop {
        tokio::select! {
            message = socket.next() => {
                let Some(message) = message else {
                    return Ok(ended(up_since, None));
                };
                let message = message.context("reading from the relay")?;
                if let Some((code, reason)) = message.as_close() {
                    let code = u16::from(code);
                    warn!(code, %reason, up_s = up_since.map(|at| at.elapsed().as_secs()), "relay closed");
                    if code == CLOSE_REPLACED {
                        warn!("the relay took a newer bot connection: is a second bot running with the same RELAY_SECRET?");
                    }
                    return Ok(ended(up_since, Some(code)));
                }
                let Some(text) = message.as_text() else {
                    continue;
                };
                if text == PONG {
                    last_pong = Instant::now();
                    continue;
                }
                let Ok(frame) = serde_json::from_str::<Incoming>(text) else {
                    warn!("the relay sent a frame that is not JSON");
                    continue;
                };
                if frame.t == WELCOME {
                    info!(down_s = down_since.elapsed().as_secs(), "relay connected; the admin pages are live");
                    up_since = Some(Instant::now());
                    continue;
                }
                if frame.t != REQUEST {
                    continue;
                }
                let (routes, answers) = (routes.clone(), answers.clone());
                let what = format!("{} {}", frame.method, redacted(&frame.path));
                tokio::spawn(async move {
                    let started = Instant::now();
                    let out = answer(routes, frame).await;
                    answers.send((what, started, out)).ok();
                });
            }
            Some((what, started, out)) = answered.recv() => {
                let took = started.elapsed();
                if out.status >= STATUS_ERROR || took > SLOW {
                    warn!(request = %what, status = out.status, ms = took.as_millis(), "relay request answered badly or slowly");
                }
                socket.send(Message::text(serde_json::to_string(&out)?)).await.context("answering through the relay")?;
            }
            _ = ping.tick() => {
                if last_pong.elapsed() > DEAD_AFTER {
                    warn!(silent_s = last_pong.elapsed().as_secs(), "the relay gave no pong; dropping the socket");
                    *down_since = Instant::now();
                    bail!("no pong");
                }
                socket.send(Message::text(PING)).await.context("pinging the relay")?;
            }
        }
    }
}

fn ended(up_since: Option<Instant>, code: Option<u16>) -> Ended {
    if code == Some(CLOSE_UNAUTHORIZED) {
        return Ended::Refused;
    }
    if up_since.is_some() { Ended::Welcomed } else { Ended::NeverWelcomed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::Services;
    use crate::services::guild::Guild;
    use std::sync::Arc;

    fn frame(method: &str, path: &str, content_type: &str, body: &str, body64: Option<String>) -> Incoming {
        Incoming {
            t: REQUEST.to_owned(),
            id: "r1".to_owned(),
            method: method.to_owned(),
            path: path.to_owned(),
            query: String::new(),
            content_type: content_type.to_owned(),
            body: body.to_owned(),
            body64,
        }
    }

    #[test]
    fn tokens_never_reach_the_log() {
        assert_eq!(redacted("/a/0123456789abcdef0123456789abcdef/players/157"), "/a/<token>/players/157");
        assert_eq!(redacted("/healthz"), "/healthz");
    }

    #[tokio::test]
    async fn a_request_frame_runs_through_the_routes_and_comes_back_a_response_frame() {
        let services = Arc::new(Services::memory(0));
        let (guild, _asks) = Guild::new(1);
        let routes = crate::web::routes(Arc::clone(&services), guild);

        let out = answer(routes.clone(), frame("GET", "/healthz", "", "", None)).await;
        assert_eq!((out.id.as_str(), out.status, out.body.as_str()), ("r1", 200, "ok"));
        assert!(out.headers.contains_key("content-security-policy"));

        // A 303 keeps its Location, so the relay can send the browser on.
        let token = services.links().issue(9, crate::services::links::Access::Staff).unwrap();
        let find = Incoming { query: "?id=157".to_owned(), ..frame("GET", &format!("/a/{token}/players/find"), "", "", None) };
        let out = answer(routes.clone(), find).await;
        assert_eq!((out.status, out.headers.get("location").map(String::as_str)), (303, Some(format!("/a/{token}/players/157").as_str())));

        // A binary body arrives as base64 and is read whole.
        let multipart = "--b\r\ncontent-disposition: form-data; name=\"picture\"; filename=\"x.png\"\r\ncontent-type: image/png\r\n\r\nnot a png\r\n--b--\r\n";
        let out = answer(routes, frame("POST", &format!("/a/{token}/characters/1/picture"), "multipart/form-data; boundary=b", "", Some(BASE64.encode(multipart)))).await;
        assert_eq!(out.status, 303);
    }

    #[tokio::test]
    async fn a_body_that_is_not_base64_is_a_500_not_a_crash() {
        let services = Arc::new(Services::memory(0));
        let (guild, _asks) = Guild::new(1);
        let routes = crate::web::routes(services, guild);
        let out = answer(routes, frame("POST", "/healthz", "image/png", "", Some("%%%".to_owned()))).await;
        assert_eq!(out.status, STATUS_ERROR);
    }
}
