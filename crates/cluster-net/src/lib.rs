//! The telemetry hub. Runs inside the desktop app on its own thread:
//!
//! - `GET /` serves the web display (WASM build of the same dashboard UI).
//! - `GET /ws` streams binary telemetry frames and JSON control messages
//!   (see [`cluster_core::wire`]).
//! - Advertises `_dashcluster._tcp` over mDNS so other desktop apps find it.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use cluster_core::SignalBus;
use cluster_core::wire::{self, ClientMsg, ServerMsg};
use futures_util::{SinkExt, StreamExt};
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

pub const DEFAULT_PORT: u16 = 5380;
pub const MDNS_SERVICE: &str = "_dashcluster._tcp.local.";

/// This machine's LAN address (picks the outgoing interface; sends nothing).
pub fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    s.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}

/// What the hub offers, updated by the app (dashboard switches, hot reloads, units).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    /// (name, JSON5 source), in the app's order.
    pub dashboards: Vec<(String, String)>,
    /// Name of the dashboard shown on the hub machine.
    pub current: String,
    /// "metric" or "imperial".
    pub units: String,
}

impl Catalog {
    fn source(&self, name: &str) -> Option<&str> {
        self.dashboards.iter().find(|(n, _)| n == name).map(|(_, s)| s.as_str())
    }
}

#[derive(Clone)]
struct AppState {
    bus: Arc<SignalBus>,
    catalog: watch::Receiver<Catalog>,
    rate_hz: f32,
    web_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone)]
pub struct HubConfig {
    pub port: u16,
    /// Max telemetry frames per second sent to each display.
    pub rate_hz: f32,
    /// Advertise over mDNS.
    pub advertise: bool,
    /// Serve the web display from this folder instead of the embedded copy (dev).
    pub web_dir: Option<std::path::PathBuf>,
}

impl Default for HubConfig {
    fn default() -> Self {
        HubConfig { port: DEFAULT_PORT, rate_hz: 60.0, advertise: true, web_dir: None }
    }
}

/// A running hub. Stops when dropped.
pub struct Hub {
    pub addr: SocketAddr,
    catalog: watch::Sender<Catalog>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
    mdns: Option<mdns_sd::ServiceDaemon>,
    clients: Arc<std::sync::atomic::AtomicUsize>,
}

impl Hub {
    pub fn start(bus: Arc<SignalBus>, catalog: Catalog, config: HubConfig) -> anyhow::Result<Hub> {
        let (cat_tx, cat_rx) = watch::channel(catalog);
        let clients = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let state =
            AppState { bus, catalog: cat_rx, rate_hz: config.rate_hz.clamp(1.0, 120.0), web_dir: config.web_dir };
        let std_listener = std::net::TcpListener::bind(("0.0.0.0", config.port))
            .map_err(|e| anyhow::anyhow!("can't listen on TCP port {}: {e}", config.port))?;
        std_listener.set_nonblocking(true)?;
        let addr = std_listener.local_addr()?;
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let counter = clients.clone();
        let thread = std::thread::Builder::new().name("hub".into()).spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt,
                Err(e) => return log::error!("hub runtime: {e}"),
            };
            rt.block_on(async move {
                let app = axum::Router::new()
                    .route("/", get(index))
                    .route("/ws", get(ws_handler))
                    .route("/{file}", get(asset))
                    .with_state((state, counter));
                let listener = match tokio::net::TcpListener::from_std(std_listener) {
                    Ok(l) => l,
                    Err(e) => return log::error!("hub listener: {e}"),
                };
                let server = axum::serve(listener, app).with_graceful_shutdown(async {
                    let _ = stop_rx.await;
                });
                if let Err(e) = server.await {
                    log::error!("hub stopped: {e}");
                }
            });
        })?;
        let mdns =
            if config.advertise { advertise(addr.port()).map_err(|e| log::warn!("mDNS: {e:#}")).ok() } else { None };
        log::info!("hub listening on {addr}");
        Ok(Hub { addr, catalog: cat_tx, shutdown: Some(stop_tx), thread: Some(thread), mdns, clients })
    }

    /// URL a phone on the same network should open.
    pub fn url(&self) -> String {
        let ip = lan_ip().map_or_else(|| "localhost".to_owned(), |ip| ip.to_string());
        format!("http://{ip}:{}/", self.addr.port())
    }

    pub fn set_catalog(&self, catalog: Catalog) {
        self.catalog.send_if_modified(|c| {
            let changed = *c != catalog;
            *c = catalog;
            changed
        });
    }

    pub fn client_count(&self) -> usize {
        self.clients.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for Hub {
    fn drop(&mut self) {
        if let Some(m) = self.mdns.take() {
            let _ = m.shutdown();
        }
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn advertise(port: u16) -> anyhow::Result<mdns_sd::ServiceDaemon> {
    let daemon = mdns_sd::ServiceDaemon::new()?;
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        })
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "dashcluster".into());
    let short = host.trim_end_matches(".local").to_owned();
    let instance = format!("DashCluster on {short}");
    let info = mdns_sd::ServiceInfo::new(MDNS_SERVICE, &instance, &format!("{short}.local."), "", port, None)?
        .enable_addr_auto();
    daemon.register(info)?;
    Ok(daemon)
}

type Shared = (AppState, Arc<std::sync::atomic::AtomicUsize>);

const FALLBACK_PAGE: &str = "<!doctype html><meta name=viewport content='width=device-width'>\
<body style='font:16px system-ui;background:#000;color:#ddd;padding:2em'>\
<h2>DashCluster hub is running</h2><p>The web display wasn't built into this copy of the app.\
 Build it with <code>scripts/build-web.sh</code> and rebuild the desktop app.</p>";

async fn index(State((state, _)): State<Shared>) -> Response {
    asset_response(&state, "index.html").unwrap_or_else(|| Html(FALLBACK_PAGE).into_response())
}

async fn asset(State((state, _)): State<Shared>, Path(file): Path<String>) -> Response {
    asset_response(&state, &file).unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "json" | "webmanifest" => "application/json",
        _ => "application/octet-stream",
    }
}

fn asset_response(state: &AppState, name: &str) -> Option<Response> {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return None;
    }
    let body: Vec<u8> = match &state.web_dir {
        Some(dir) => std::fs::read(dir.join(name)).ok()?,
        None => WEB_ASSETS.iter().find(|(n, _)| *n == name)?.1.to_vec(),
    };
    Some(([(header::CONTENT_TYPE, content_type(name)), (header::CACHE_CONTROL, "no-cache")], body).into_response())
}

async fn ws_handler(ws: WebSocketUpgrade, State((state, counter)): State<Shared>) -> Response {
    ws.on_upgrade(move |socket| async move {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Err(e) = client(socket, state).await {
            log::debug!("display disconnected: {e}");
        }
        counter.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    })
}

fn text(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).expect("serializable").into())
}

fn layout_msg(cat: &Catalog, name: &str) -> Option<Message> {
    cat.source(name).map(|src| text(&ServerMsg::Layout { name: name.to_owned(), source: src.to_owned() }))
}

async fn client(socket: WebSocket, mut state: AppState) -> anyhow::Result<()> {
    let (mut tx, mut rx) = socket.split();
    // `None` = follow the hub's current dashboard.
    let mut chosen: Option<String> = None;
    {
        let cat = state.catalog.borrow_and_update().clone();
        tx.send(text(&ServerMsg::Hello {
            version: wire::PROTOCOL_VERSION,
            dashboards: cat.dashboards.iter().map(|(n, _)| n.clone()).collect(),
            current: cat.current.clone(),
            units: cat.units.clone(),
        }))
        .await?;
        if let Some(m) = layout_msg(&cat, &cat.current) {
            tx.send(m).await?;
        }
    }
    let mut tick = tokio::time::interval(Duration::from_secs_f32(1.0 / state.rate_hz));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut status_tick = tokio::time::interval(Duration::from_secs(1));
    let mut last_seq = u64::MAX;
    let mut last_frame_count = 0u64;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let count = state.bus.publish_count();
                if count != last_frame_count {
                    last_frame_count = count;
                    let frame = state.bus.latest();
                    if frame.snapshot.seq != last_seq || count == 1 {
                        last_seq = frame.snapshot.seq;
                        tx.send(Message::Binary(wire::encode_frame(&frame.snapshot).into())).await?;
                    }
                }
            }
            _ = status_tick.tick() => {
                let st = state.bus.status();
                let stale = state.bus.age_secs().is_none_or(|a| a > 0.5);
                let s = match &st.state {
                    cluster_core::SourceState::Idle => "idle",
                    cluster_core::SourceState::Waiting => "waiting",
                    cluster_core::SourceState::Live if stale => "waiting",
                    cluster_core::SourceState::Live => "live",
                    cluster_core::SourceState::Error(_) => "error",
                };
                let detail = match &st.state { cluster_core::SourceState::Error(e) => e.clone(), _ => st.detail.clone() };
                tx.send(text(&ServerMsg::Status { source: st.source.clone(), state: s.into(), detail })).await?;
            }
            changed = state.catalog.changed() => {
                if changed.is_err() { break; }
                let cat = state.catalog.borrow_and_update().clone();
                tx.send(text(&ServerMsg::Hello {
                    version: wire::PROTOCOL_VERSION,
                    dashboards: cat.dashboards.iter().map(|(n, _)| n.clone()).collect(),
                    current: cat.current.clone(),
                    units: cat.units.clone(),
                })).await?;
                let name = chosen.clone().unwrap_or_else(|| cat.current.clone());
                if let Some(m) = layout_msg(&cat, &name) {
                    tx.send(m).await?;
                }
            }
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        if let Ok(ClientMsg::Select { name }) = serde_json::from_str::<ClientMsg>(&t) {
                            let cat = state.catalog.borrow().clone();
                            chosen = if name.is_empty() { None } else { Some(name.clone()) };
                            let name = chosen.clone().unwrap_or_else(|| cat.current.clone());
                            if let Some(m) = layout_msg(&cat, &name) {
                                tx.send(m).await?;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cluster_core::{SignalId, Snapshot};

    #[test]
    fn streams_layout_and_frames_to_a_display() {
        let bus = SignalBus::new();
        let catalog = Catalog {
            dashboards: vec![("race".into(), "{race}".into()), ("gt".into(), "{gt}".into())],
            current: "race".into(),
            units: "metric".into(),
        };
        let hub =
            Hub::start(bus.clone(), catalog.clone(), HubConfig { port: 0, advertise: false, ..Default::default() })
                .unwrap();
        let url = format!("ws://127.0.0.1:{}/ws", hub.addr.port());
        let (mut ws, _) = tungstenite::connect(url).unwrap();

        let read_text = |ws: &mut tungstenite::WebSocket<_>| loop {
            if let tungstenite::Message::Text(t) = ws.read().unwrap() {
                let m: ServerMsg = serde_json::from_str(&t).unwrap();
                if !matches!(m, ServerMsg::Status { .. }) {
                    return m;
                }
            }
        };
        assert!(matches!(read_text(&mut ws), ServerMsg::Hello { current, .. } if current == "race"));
        assert_eq!(read_text(&mut ws), ServerMsg::Layout { name: "race".into(), source: "{race}".into() });

        let mut s = Snapshot::new();
        s.seq = 9;
        s.set(SignalId::Rpm, 4321.0);
        bus.publish(s.clone());
        let frame = loop {
            if let tungstenite::Message::Binary(b) = ws.read().unwrap() {
                break wire::decode_frame(&b).unwrap();
            }
        };
        assert_eq!(frame, s);

        // Choosing a dashboard, then the hub switching: the display keeps its choice.
        ws.send(tungstenite::Message::Text(
            serde_json::to_string(&ClientMsg::Select { name: "gt".into() }).unwrap().into(),
        ))
        .unwrap();
        assert_eq!(read_text(&mut ws), ServerMsg::Layout { name: "gt".into(), source: "{gt}".into() });
        hub.set_catalog(Catalog { current: "gt".into(), ..catalog });
        assert!(matches!(read_text(&mut ws), ServerMsg::Hello { .. }));
        assert_eq!(read_text(&mut ws), ServerMsg::Layout { name: "gt".into(), source: "{gt}".into() });
        assert_eq!(hub.client_count(), 1);

        // Fallback page is served even without the web build.
        let page = std::net::TcpStream::connect(hub.addr).and_then(|mut s| {
            use std::io::{Read, Write};
            s.write_all(b"GET / HTTP/1.0\r\n\r\n")?;
            let mut out = String::new();
            s.read_to_string(&mut out)?;
            Ok(out)
        });
        assert!(page.unwrap().contains("200 OK"));
    }
}
