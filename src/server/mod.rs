use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use axum::Router;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::config::Config;
use crate::store::Store;
use crate::util;

mod api;
mod middleware;


use crate::ServeArgs;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<RwLock<Config>>,
    /// Swappable so the storage folder can change while the server runs.
    pub store: Arc<RwLock<Arc<Store>>>,
    pub sessions: Arc<Mutex<HashMap<String, Arc<UploadSession>>>>,
    pub events: broadcast::Sender<LiveEvent>,
    pub started: i64,
    pub paused: Arc<AtomicBool>,
}

impl AppState {
    pub fn store(&self) -> Arc<Store> {
        self.store.read().unwrap().clone()
    }
    pub fn set_store(&self, store: Arc<Store>) {
        *self.store.write().unwrap() = store;
    }
    pub fn settings(&self) -> crate::config::Settings {
        self.cfg.read().unwrap().settings.clone()
    }
    pub fn token(&self) -> String {
        self.cfg.read().unwrap().token.clone()
    }
    pub fn device_name(&self) -> String {
        self.cfg.read().unwrap().device_name.clone()
    }
    pub fn emit(&self, event: LiveEvent) {
        let _ = self.events.send(event);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveEvent {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub device: String,
    pub size: u64,
    pub at: i64,
}

impl LiveEvent {
    pub fn new(kind: &str, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            detail: detail.into(),
            device: String::new(),
            size: 0,
            at: util::now_ms(),
        }
    }
    pub fn with_device(mut self, device: impl Into<String>, size: u64) -> Self {
        self.device = device.into();
        self.size = size;
        self
    }
}

pub struct UploadSession {
    pub meta: SessionMeta,
    pub tmp: PathBuf,
    pub offset: tokio::sync::Mutex<u64>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub name: String,
    pub rel_path: String,
    pub device: String,
    pub platform: String,
    pub size: u64,
    pub mtime_ms: i64,
    pub resume_key: String,
}

pub const CHUNK_SIZE: u64 = 8 * 1024 * 1024;

pub fn router(state: AppState) -> Router {
    api::router(state)
}

pub fn run(mut cfg: Config, args: ServeArgs) -> Result<()> {
    cfg.port = args.port;
    if let Some(name) = args.name.clone() {
        cfg.device_name = name;
    }
    if args.open {
        cfg.token.clear();
    }
    cfg.ensure_dirs()?;
    cfg.save()?;

    let store = Arc::new(Store::open(cfg.root.clone())?);
    let (tx, _rx) = broadcast::channel(256);
    let state = AppState {
        paused: Arc::new(AtomicBool::new(cfg.settings.paused)),
        started: util::now_ms(),
        cfg: Arc::new(RwLock::new(cfg.clone())),
        store: Arc::new(RwLock::new(store)),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        events: tx,
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("phonesync")
        .build()
        .context("starting async runtime")?;

    runtime.block_on(async move {
        // Load any half-finished uploads so a phone can resume after a restart.
        api::restore_sessions(&state);

        let mdns = if let Some(handle) = start_mdns(&cfg, args.port) {
            Some(handle)
        } else {
            tracing::warn!("mDNS unavailable, phone clients must use the IP address");
            None
        };

        let app = router(state.clone());
        let addr: SocketAddr = format!("{}:{}", args.bind, args.port)
            .parse()
            .with_context(|| format!("invalid bind address {}", args.bind))?;
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("port {} is already in use", args.port))?;

        print_banner(&cfg, args.port, args.open, args.no_qr);

        if args.dashboard {
            if let Some(url) = dashboard_url(&cfg, args.port) {
                util::os_open(std::path::Path::new(&url));
            }
        }

        let shutdown = async {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("shutting down");
        };

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        .context("http server failed")?;

        if let Some(handle) = mdns {
            handle.shutdown().ok();
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn dashboard_url(cfg: &Config, port: u16) -> Option<String> {
    Some(format!(
        "http://{}:{}/dashboard?k={}",
        util::lan_ip(),
        port,
        cfg.token
    ))
}

pub fn sync_url(cfg: &Config, port: u16) -> String {
    let base = format!("http://{}:{}/", util::lan_ip(), port);
    if cfg.token.is_empty() {
        base
    } else {
        format!("{base}?k={}", cfg.token)
    }
}

fn print_banner(cfg: &Config, port: u16, open: bool, no_qr: bool) {
    let url = sync_url(cfg, port);
    let width = 62;
    let line = "─".repeat(width);
    println!("\n┌{line}┐");
    println!("│ PhoneSync  ready on {}", util::lan_ip());
    println!("├{line}┤");
    println!("│ Phone:     {url}");
    println!("│ Dashboard: http://localhost:{port}/dashboard");
    println!("│ Storage:   {}", cfg.root.display());
    if open {
        println!("│ ⚠  Token disabled: anyone on this Wi-Fi can upload.");
    } else {
        println!("│ Token:     {}  (already inside the phone URL)", cfg.token);
    }
    println!("└{line}┘\n");

    if !no_qr && !open {
        if let Ok(code) = qrcode::QrCode::new(url.as_bytes()) {
            let art = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(true)
                .module_dimensions(1, 1)
                .build();
            println!("Scan with your phone camera:\n\n{art}\n");
        }
    }
}

pub fn print_info(cfg: &Config, port: u16) -> Result<()> {
    println!("phone url: {}", sync_url(cfg, port));
    println!("dashboard: http://localhost:{port}/dashboard");
    println!("root:      {}", cfg.root.display());
    println!("token:     {}", cfg.token);
    println!("lan ip:    {}", util::lan_ip());
    Ok(())
}

struct MdnsHandle {
    daemon: mdns_sd::ServiceDaemon,
    fullname: String,
}

impl MdnsHandle {
    fn shutdown(self) -> Result<()> {
        self.daemon.unregister(&self.fullname)?;
        self.daemon.shutdown()?;
        Ok(())
    }
}

fn start_mdns(cfg: &Config, port: u16) -> Option<MdnsHandle> {
    use mdns_sd::{ServiceDaemon, ServiceInfo};

    let daemon = ServiceDaemon::new().ok()?;
    let instance = format!("{}", cfg.device_name.replace(['.', ' '], "-"));
    let host = format!("{}.local.", cfg.device_name.replace(' ', "-").to_lowercase());
    let ip = util::lan_ip();
    let props = HashMap::from([
        ("v".to_string(), env!("CARGO_PKG_VERSION").to_string()),
        ("name".to_string(), cfg.device_name.clone()),
        ("auth".to_string(), (!cfg.token.is_empty()).to_string()),
    ]);
    let info = ServiceInfo::new(
        "_phonesync._tcp.local.",
        &instance,
        &host,
        ip.as_str(),
        port,
        Some(props),
    )
    .ok()?;
    let fullname = info.get_fullname().to_string();
    daemon.register(info).ok()?;
    tracing::info!("advertising _phonesync._tcp on {ip}:{port}");
    Some(MdnsHandle { daemon, fullname })
}
