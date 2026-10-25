use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware as mw;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use super::{middleware, AppState, LiveEvent, SessionMeta, UploadSession, CHUNK_SIZE};
use crate::config::{Config, Organize, Settings};
use crate::server::{dashboard_url, sync_url};
use crate::store::{NewFile, Placed};
use crate::ui;
use crate::util;

const MAX_UPLOAD: u64 = 64 * 1024 * 1024 * 1024;

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    fn bad(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}

impl From<tokio::task::JoinError> for ApiError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(ui::index))
        .route("/dashboard", get(ui::dashboard))
        .route("/assets/{file}", get(ui::asset))
        .route("/api/info", get(info))
        .route("/api/stats", get(stats))
        .route("/api/devices", get(devices))
        .route("/api/files", get(files))
        .route("/api/events", get(events))
        .route("/api/qr.svg", get(qr))
        .route("/api/raw/{id}", get(raw_file))
        .route("/api/settings", get(read_settings).patch(write_settings))
        .route("/api/session", post(create_session))
        .route("/api/session/{id}", put(put_chunk).delete(cancel_session))
        .route("/api/session/{id}/complete", post(complete))
        .route("/api/actions/open-folder", post(open_folder))
        .route("/api/actions/reveal", post(reveal))
        .route("/api/ping", post(ping))
        .route("/healthz", get(|| async { "ok" }))
        .layer(mw::from_fn_with_state(state.clone(), middleware::require_token))
        .with_state(state)
}

// ---------------------------------------------------------------- info / stats

async fn info(State(st): State<AppState>, ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Json<Value> {
    let cfg = st.cfg.read().unwrap().clone();
    Json(json!({
        "app": "phonesync",
        "version": env!("CARGO_PKG_VERSION"),
        "name": cfg.device_name,
        "auth": !cfg.token.is_empty(),
        "local": peer.ip().is_loopback(),
        "root": cfg.root.display().to_string(),
        "port": cfg.port,
        "started": st.started,
        "sync_url": sync_url(&cfg, cfg.port),
        "dashboard_url": dashboard_url(&cfg, cfg.port),
        "chunk_size": CHUNK_SIZE,
        "paused": st.paused.load(Ordering::Relaxed),
        "os": std::env::consts::OS,
    }))
}

async fn stats(State(st): State<AppState>) -> Json<Value> {
    let stats = st.store().stats();
    let (paused, organize) = {
        let cfg = st.cfg.read().unwrap();
        (
            st.paused.load(Ordering::Relaxed),
            format!("{:?}", cfg.settings.organize).to_lowercase(),
        )
    };
    Json(json!({
        "files": stats.files,
        "bytes": stats.bytes,
        "human_total": stats.human_total,
        "today_files": stats.today_files,
        "today_bytes": stats.today_bytes,
        "devices": stats.devices,
        "free_space": stats.free_space,
        "human_free": stats.human_free,
        "root": stats.root,
        "days": stats.days,
        "paused": paused,
        "organize": organize,
        "uptime_ms": util::now_ms() - st.started,
    }))
}

async fn devices(State(st): State<AppState>) -> Json<Value> {
    Json(json!({ "devices": st.store().devices() }))
}

#[derive(Deserialize)]
struct FilesQuery {
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    q: String,
}

fn default_limit() -> usize {
    60
}

async fn files(State(st): State<AppState>, Query(q): Query<FilesQuery>) -> Json<Value> {
    let limit = q.limit.clamp(1, 500);
    Json(json!({ "files": st.store().files(limit, &q.q) }))
}

// --------------------------------------------------------------------- events

async fn qr(State(st): State<AppState>) -> Response {
    let cfg = st.cfg.read().unwrap().clone();
    match ui::qr_svg(&sync_url(&cfg, cfg.port)) {
        Some(svg) => (
            [
                (axum::http::header::CONTENT_TYPE, "image/svg+xml"),
                (axum::http::header::CACHE_CONTROL, "no-cache"),
            ],
            svg,
        )
            .into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "qr failed").into_response(),
    }
}

/// Serves the original bytes of a received image so both UIs can show a
/// thumbnail without a decoding library. Capped so a 4 GB video never streams
/// through the dashboard.
async fn raw_file(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let Some(record) = st.store().find(&id) else {
        return (StatusCode::NOT_FOUND, "no such file").into_response();
    };
    const PREVIEW_MAX: u64 = 24 * 1024 * 1024;
    if record.size > PREVIEW_MAX {
        return (StatusCode::PAYLOAD_TOO_LARGE, "too large to preview").into_response();
    }
    let path = st.store().abs_path(&record);
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = match util::ext_of(&record.name).as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "avif" => "image/avif",
                "bmp" => "image/bmp",
                "svg" => "image/svg+xml",
                "mp4" | "m4v" => "video/mp4",
                "webm" => "video/webm",
                _ => "application/octet-stream",
            };
            (
                [
                    (axum::http::header::CONTENT_TYPE, mime),
                    (axum::http::header::CACHE_CONTROL, "private, max-age=600"),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "file is gone").into_response(),
    }
}

async fn events(
    State(st): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = st.events.subscribe();
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let sse = SseEvent::default()
                        .json_data(event)
                        .unwrap_or_else(|_| SseEvent::default().data("{}"));
                    return Some((Ok::<_, Infallible>(sse), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

// ------------------------------------------------------------------- settings

async fn read_settings(State(st): State<AppState>) -> Json<Value> {
    let cfg = st.cfg.read().unwrap().clone();
    Json(json!({
        "organize": cfg.settings.organize,
        "skip_junk": cfg.settings.skip_junk,
        "sidecars": cfg.settings.sidecars,
        "paused": st.paused.load(Ordering::Relaxed),
        "root": cfg.root.display().to_string(),
        "device_name": cfg.device_name,
        "port": cfg.port,
        "token": cfg.token,
        "sync_url": sync_url(&cfg, cfg.port),
    }))
}

#[derive(Deserialize)]
struct SettingsPatch {
    organize: Option<Organize>,
    skip_junk: Option<bool>,
    sidecars: Option<bool>,
    paused: Option<bool>,
    device_name: Option<String>,
    /// New storage folder. Existing files stay where they are; the receiver
    /// simply starts writing to the new place.
    root: Option<String>,
}

async fn write_settings(
    State(st): State<AppState>,
    Json(patch): Json<SettingsPatch>,
) -> Result<Json<Value>, ApiError> {
    let mut new_root: Option<PathBuf> = None;
    {
        let mut cfg = st.cfg.write().unwrap();
        if let Some(organize) = patch.organize {
            cfg.settings.organize = organize;
        }
        if let Some(skip) = patch.skip_junk {
            cfg.settings.skip_junk = skip;
        }
        if let Some(sidecars) = patch.sidecars {
            cfg.settings.sidecars = sidecars;
        }
        if let Some(paused) = patch.paused {
            cfg.settings.paused = paused;
            st.paused.store(paused, Ordering::Relaxed);
        }
        if let Some(name) = patch.device_name {
            if !name.trim().is_empty() {
                cfg.device_name = name.trim().to_string();
            }
        }
        if let Some(root) = patch.root {
            let raw = root.trim().trim_matches('"');
            if raw.is_empty() {
                return Err(ApiError::bad("storage folder cannot be empty"));
            }
            let path = PathBuf::from(raw);
            if path != cfg.root {
                cfg.root = path.clone();
                cfg.settings.storage_root = Some(path.display().to_string());
                new_root = Some(path);
            }
        }
        cfg.save()?;
    }

    if let Some(root) = new_root {
        // An upload in flight writes into the old folder; make the user finish
        // or cancel it rather than silently splitting a file across roots.
        if !st.sessions.lock().unwrap().is_empty() {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "an upload is in progress — finish it, then change the folder",
            ));
        }
        std::fs::create_dir_all(root.join(".phonesync").join("sessions"))
            .map_err(|err| ApiError::bad(format!("cannot use that folder: {err}")))?;
        let store = crate::store::Store::open(root.clone())
            .map_err(|err| ApiError::bad(format!("cannot read that folder: {err}")))?;
        st.set_store(Arc::new(store));
        st.emit(
            LiveEvent::new("device", "Storage folder changed", root.display().to_string())
                .with_device("system", 0),
        );
    }

    Ok(read_settings(State(st)).await)
}

// ------------------------------------------------------------------- sessions

#[derive(Deserialize)]
struct CreateReq {
    name: String,
    #[serde(default)]
    rel_path: String,
    size: u64,
    #[serde(default)]
    mtime_ms: i64,
    #[serde(default)]
    device: String,
    #[serde(default)]
    platform: String,
}

#[axum::debug_handler]
async fn create_session(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<CreateReq>,
) -> Result<Json<Value>, ApiError> {
    if st.paused.load(Ordering::Relaxed) {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "the laptop paused sync",
        ));
    }
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::bad("file name is required"));
    }
    if req.size > MAX_UPLOAD {
        return Err(ApiError::bad("file is larger than 64 GB"));
    }
    let device = if req.device.trim().is_empty() {
        "Phone".to_string()
    } else {
        req.device.trim().to_string()
    };

    let resume_key = format!(
        "{}-{}",
        util::sanitize_component(&device),
        &util::sha256_bytes(
            format!(
                "{}|{}|{}|{}",
                req.rel_path, name, req.size, req.mtime_ms
            )
            .as_bytes()
        )[..16]
    );

    // Resume an interrupted upload of the same file.
    let existing = {
        let map = st.sessions.lock().unwrap();
        map.values()
            .find(|s| s.meta.resume_key == resume_key)
            .cloned()
    };
    if let Some(existing) = existing {
        let offset = *existing.offset.lock().await;
        return Ok(Json(json!({
            "id": existing.meta.id,
            "offset": offset,
            "resumed": true,
            "chunk_size": CHUNK_SIZE,
        })));
    }

    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let meta = SessionMeta {
        id: id.clone(),
        name: name.clone(),
        rel_path: normalize_rel(&req.rel_path, &name),
        device: device.clone(),
        platform: req.platform.clone(),
        size: req.size,
        mtime_ms: if req.mtime_ms > 0 {
            req.mtime_ms
        } else {
            util::now_ms()
        },
        resume_key,
    };

    let tmp = st.cfg.read().unwrap().sessions_dir().join(format!("{id}.part"));
    tokio::fs::File::create(&tmp).await?;
    let meta_file = tmp.with_extension("json");
    tokio::fs::write(&meta_file, serde_json::to_vec(&meta)?).await?;

    st.sessions.lock().unwrap().insert(
        id.clone(),
        Arc::new(UploadSession {
            meta: meta.clone(),
            tmp,
            offset: Mutex::new(0),
        }),
    );

    st.store()
        .record_device(&device, &peer.ip().to_string(), &req.platform);

    Ok(Json(json!({
        "id": id,
        "offset": 0,
        "resumed": false,
        "chunk_size": CHUNK_SIZE,
    })))
}

async fn put_chunk(
    State(st): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<Value>, ApiError> {
    let session = st
        .sessions
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| ApiError::not_found("unknown upload session"))?;

    let mut offset = session.offset.lock().await;
    let requested: u64 = headers
        .get("x-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(*offset);

    if requested != *offset {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("expected offset {offset}, got {requested}"),
        ));
    }

    let mut file = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&session.tmp)
        .await?;
    let mut written = 0u64;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| ApiError::bad(e.to_string()))?;
        if bytes.is_empty() {
            continue;
        }
        written += bytes.len() as u64;
        if *offset + written > session.meta.size {
            return Err(ApiError::bad("chunk exceeds declared file size"));
        }
        file.write_all(&bytes).await?;
    }
    file.flush().await?;
    *offset += written;

    Ok(Json(json!({ "offset": *offset, "written": written })))
}

async fn cancel_session(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let removed = st.sessions.lock().unwrap().remove(&id);
    if let Some(session) = removed {
        let _ = tokio::fs::remove_file(&session.tmp).await;
        let _ = tokio::fs::remove_file(session.tmp.with_extension("json")).await;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn complete(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let session = st
        .sessions
        .lock()
        .unwrap()
        .remove(&id)
        .ok_or_else(|| ApiError::not_found("unknown upload session"))?;

    let offset = *session.offset.lock().await;
    if offset != session.meta.size {
        // Put it back so the phone can keep sending.
        let checkpoint = offset;
        st.sessions.lock().unwrap().insert(id, session);
        return Err(ApiError::bad(format!(
            "expected {} bytes but received {checkpoint}",
            "the full file"
        )));
    }

    let settings: Settings = st.settings();
    let store = st.store();
    let tmp = session.tmp.clone();
    let meta = session.meta.clone();

    let placed = tokio::task::spawn_blocking(move || -> anyhow::Result<Placed> {
        let hash = util::sha256_blocking(&tmp)?;
        store.place(
            &tmp,
            NewFile {
                name: meta.name.clone(),
                rel: meta.rel_path.clone(),
                device: meta.device.clone(),
                size: meta.size,
                mtime: meta.mtime_ms,
                sha256: hash,
                ip: peer.ip().to_string(),
                platform: meta.platform.clone(),
            },
            settings.organize,
            settings.sidecars,
        )
    })
    .await??;

    let _ = tokio::fs::remove_file(session.tmp.with_extension("json")).await;

    let (status, record) = match placed {
        Placed::Stored(r) => ("stored", r),
        Placed::Duplicate(r) => ("duplicate", r),
    };

    st.emit(
        LiveEvent::new(
            status,
            if status == "stored" {
                "New file"
            } else {
                "Already had this one"
            },
            format!("{} → {}", record.name, record.rel),
        )
        .with_device(record.device.clone(), record.size),
    );

    Ok(Json(json!({
        "status": status,
        "id": record.id,
        "name": record.name,
        "device": record.device,
        "size": record.size,
        "rel": record.rel,
        "path": st.store().abs_path(&record).display().to_string(),
        "deduped": status == "duplicate",
        "days": 0,
    })))
}

// -------------------------------------------------------------------- actions

async fn open_folder(State(st): State<AppState>) -> Json<Value> {
    let root = st.cfg.read().unwrap().root.clone();
    util::os_open(&root);
    Json(json!({ "ok": true }))
}

#[derive(Deserialize)]
struct IdReq {
    id: String,
}

async fn reveal(State(st): State<AppState>, Json(req): Json<IdReq>) -> Result<Json<Value>, ApiError> {
    let record = st
        .store()
        .find(&req.id)
        .ok_or_else(|| ApiError::not_found("no such file"))?;
    let path = st.store().abs_path(&record);
    util::reveal_in_folder(&path);
    Ok(Json(json!({ "ok": true, "path": path.display().to_string() })))
}

#[derive(Deserialize)]
struct PingReq {
    #[serde(default)]
    device: String,
    #[serde(default)]
    platform: String,
}

async fn ping(
    State(st): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<PingReq>,
) -> Json<Value> {
    let device = if req.device.trim().is_empty() {
        "Phone".to_string()
    } else {
        req.device.trim().to_string()
    };
    st.store()
        .record_device(&device, &peer.ip().to_string(), &req.platform);
    st.emit(
        LiveEvent::new("device", "Device connected", device.clone()).with_device(device, 0),
    );
    Json(json!({ "ok": true, "server": st.device_name() }))
}

// ------------------------------------------------------------------- restore

/// Rebuild the in-memory session table from disk so interrupted uploads resume.
pub fn restore_sessions(st: &AppState) {
    let dir = st.cfg.read().unwrap().sessions_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut restored = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_slice::<SessionMeta>(&bytes) else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        let tmp = path.with_extension("part");
        let offset = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        if !tmp.exists() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        st.sessions.lock().unwrap().insert(
            meta.id.clone(),
            Arc::new(UploadSession {
                meta,
                tmp,
                offset: Mutex::new(offset),
            }),
        );
        restored += 1;
    }
    if restored > 0 {
        tracing::info!("resumable uploads restored: {restored}");
    }
}

fn normalize_rel(rel: &str, name: &str) -> String {
    let rel = rel.trim();
    if rel.is_empty() || rel.ends_with('/') {
        return name.to_string();
    }
    rel.replace('\\', "/")
}

pub fn _unused(_: PathBuf, _: Config) {}
