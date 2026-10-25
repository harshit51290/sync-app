use std::collections::HashSet;
use std::io::{SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::config::Config;
use crate::discover;
use crate::util;
use crate::SendArgs;

const CHUNK: usize = 8 * 1024 * 1024;

#[derive(Deserialize)]
struct SessionResp {
    id: String,
    #[serde(default)]
    offset: u64,
}

#[derive(Deserialize)]
struct ChunkResp {
    offset: u64,
}

#[derive(Deserialize)]
struct CompleteResp {
    status: String,
    rel: String,
}

struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
    device: String,
    platform: String,
}

pub fn run(cfg: Config, args: SendArgs) -> Result<()> {
    let _ = &cfg;
    let base = resolve_base(&args.to)?;
    let token = args
        .token
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| std::env::var("PHONESYNC_TOKEN").ok())
        .unwrap_or_default();
    let device = args
        .device
        .clone()
        .or_else(|| std::env::var("PHONESYNC_DEVICE").ok())
        .unwrap_or_else(|| {
            if cfg!(target_os = "android") {
                "Android phone".to_string()
            } else {
                crate::config::hostname()
            }
        });

    println!("→ receiver {base}");
    if token.is_empty() {
        println!("  no token given; if the laptop rejects the upload, add --token <value>");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(2)
        .build()?;

    let client = Arc::new(Client {
        http,
        base,
        token,
        device,
        platform: if cfg!(target_os = "android") {
            "android".into()
        } else {
            std::env::consts::OS.to_string()
        },
    });

    rt.block_on(async move {
        client.ping().await;
        let files = collect(&args.paths, &args.only)?;
        if files.is_empty() {
            println!("nothing to send");
            return Ok(());
        }
        println!("{} file(s) to consider", files.len());
        if args.dry_run {
            for (path, rel) in &files {
                println!("  {rel}  ({})", util::human_bytes(std::fs::metadata(path)?.len()));
            }
            return Ok(());
        }

        let state_path = args
        .state
        .clone()
        .or_else(|| args.watch.then(crate::config::client_state_file));
    let state = Arc::new(StateFile::load(state_path));
        let mut sent = 0usize;
        let mut skipped = 0usize;
        for (path, rel) in &files {
            if state.contains(path) {
                skipped += 1;
                continue;
            }
            match client.send_file(path, rel).await {
                Ok(outcome) => {
                    if outcome.0 == "duplicate" {
                        skipped += 1;
                    } else {
                        sent += 1;
                    }
                    state.mark(path);
                }
                Err(err) => eprintln!("✗ {} — {err:#}", rel),
            }
        }
        state.flush();
        println!("\n{sent} sent, {skipped} already there");

        if args.watch {
            watch_loop(client, args, state).await?;
        }
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn resolve_base(to: &str) -> Result<String> {
    let to = to.trim();
    if to.starts_with("http://") || to.starts_with("https://") {
        return Ok(to.trim_end_matches('/').to_string());
    }
    if to == "discover" || to.is_empty() {
        println!("searching the local network…");
        let found = discover::browse(4, None)?;
        let best = found
            .first()
            .context("no receiver answered. Start `phonesync serve` on the laptop, or pass --to http://ip:port")?;
        println!("found {} at {}", best.name, best.url());
        return Ok(best.url());
    }
    // A bare host:port is a URL in disguise.
    if to.contains(':') && !to.contains('/') {
        return Ok(format!("http://{to}"));
    }
    // Otherwise treat it as a receiver name from mDNS.
    println!("searching for “{to}”…");
    let found = discover::browse(4, Some(to))?;
    let best = found
        .first()
        .with_context(|| format!("no receiver named “{to}” found on this network"))?;
    println!("found {} at {}", best.name, best.url());
    Ok(best.url())
}

impl Client {
    async fn ping(&self) {
        let body = serde_json::json!({ "device": self.device, "platform": self.platform });
        let _ = self
            .http
            .post(format!("{}/api/ping", self.base))
            .header("x-sync-key", &self.token)
            .json(&body)
            .send()
            .await;
    }

    async fn send_file(&self, path: &Path, rel: &str) -> Result<(String, u64)> {
        let meta = tokio::fs::metadata(path).await?;
        let size = meta.len();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let session: SessionResp = self
            .post_json(
                "/api/session",
                &serde_json::json!({
                    "name": name,
                    "rel_path": rel,
                    "size": size,
                    "mtime_ms": mtime,
                    "device": self.device,
                    "platform": self.platform,
                }),
            )
            .await
            .with_context(|| format!("opening upload for {rel}"))?;

        let mut file = tokio::fs::File::open(path).await?;
        let mut offset = session.offset;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset)).await?;
            print!("\r  {rel}  resuming at {}", util::human_bytes(offset));
            std::io::stdout().flush().ok();
        }
        let mut buf = vec![0u8; CHUNK];
        let mut last_pct = 0u64;
        while offset < size {
            let want = ((size - offset) as usize).min(CHUNK);
            let chunk = &mut buf[..want];
            file.read_exact(chunk).await?;
            offset = self
                .put_chunk(&session.id, offset, chunk.to_vec())
                .await
                .with_context(|| format!("uploading {rel} at offset {offset}"))?;
            if size > 0 {
                let pct = offset * 100 / size;
                if pct / 10 != last_pct / 10 || offset == size {
                    last_pct = pct;
                    print!("\r  {rel}  {pct}%  {}", util::human_bytes(offset));
                    std::io::stdout().flush().ok();
                }
            }
        }

        let done: CompleteResp = self
            .post_json(&format!("/api/session/{}/complete", session.id), &serde_json::json!({}))
            .await
            .with_context(|| format!("finishing {rel}"))?;
        println!(
            "\r  {rel}  {} → {}",
            if done.status == "duplicate" { "already there" } else { "saved" },
            done.rel
        );
        Ok((done.status, size))
    }

    async fn put_chunk(&self, id: &str, offset: u64, bytes: Vec<u8>) -> Result<u64> {
        let mut delay = Duration::from_millis(400);
        let mut last: Option<anyhow::Error> = None;
        for attempt in 0..5 {
            let res = self
                .http
                .put(format!("{}/api/session/{id}", self.base))
                .header("x-sync-key", &self.token)
                .header("x-offset", offset.to_string())
                .body(bytes.clone())
                .send()
                .await;
            match res {
                Ok(resp) if resp.status().is_success() => {
                    let body: ChunkResp = resp.json().await?;
                    return Ok(body.offset);
                }
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    last = Some(anyhow::anyhow!("{status}: {text}"));
                    if status.as_u16() == 401 {
                        break;
                    }
                }
                Err(err) => last = Some(err.into()),
            }
            if attempt < 4 {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
        }
        Err(last.unwrap_or_else(|| anyhow::anyhow!("upload failed")))
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .header("x-sync-key", &self.token)
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("{status}: {text}");
        }
        Ok(resp.json().await?)
    }
}

fn collect(paths: &[PathBuf], only: &[String]) -> Result<Vec<(PathBuf, String)>> {
    let mut out = Vec::new();
    let filter: HashSet<String> = only.iter().map(|e| e.to_ascii_lowercase()).collect();

    for root in paths {
        let root = root.canonicalize().unwrap_or_else(|_| root.clone());
        if root.is_file() {
            out.push((root.clone(), file_name_of(&root)));
            continue;
        }
        if !root.exists() {
            eprintln!("skipping missing path {}", root.display());
            continue;
        }
        let prefix = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                !(e.file_type().is_dir() && util::is_junk(Path::new(&name), &name))
            })
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if util::is_junk(path, &name) {
                continue;
            }
            if !filter.is_empty() {
                let ext = util::ext_of(&name);
                if !filter.contains(&ext) {
                    continue;
                }
            }
            let rel = path
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| name.clone());
            let rel = format!("{prefix}/{rel}");
            out.push((path.to_path_buf(), rel));
        }
    }
    Ok(out)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into())
}

/* -------------------------------------------------------------------- watch */

#[derive(Default, Serialize, Deserialize)]
struct StateData {
    #[serde(default)]
    files: HashSet<String>,
}

struct StateFile {
    path: Option<PathBuf>,
    files: std::sync::Mutex<HashSet<String>>,
}

impl StateFile {
    fn load(path: Option<PathBuf>) -> Self {
        let data: StateData = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            path,
            files: std::sync::Mutex::new(data.files),
        }
    }

    fn key(path: &Path) -> String {
        let meta = std::fs::metadata(path).ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        format!("{}|{size}|{mtime}", path.display())
    }

    fn contains(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains(&Self::key(path))
    }

    fn mark(&self, path: &Path) {
        self.files.lock().unwrap().insert(Self::key(path));
    }

    fn flush(&self) {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let data = StateData {
                files: self.files.lock().unwrap().clone(),
            };
            if let Ok(bytes) = serde_json::to_vec(&data) {
                let tmp = path.with_extension("json.tmp");
                let _ = std::fs::write(&tmp, bytes);
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }
}

async fn watch_loop(client: Arc<Client>, args: SendArgs, state: Arc<StateFile>) -> Result<()> {
    use notify::{RecursiveMode, Watcher};

    println!("\nwatching for new files — Ctrl+C to stop");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<PathBuf>(512);

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            for path in event.paths {
                let _ = tx.blocking_send(path);
            }
        }
    })?;
    for root in &args.paths {
        if root.is_dir() {
            watcher.watch(root, RecursiveMode::Recursive)?;
        }
    }

    // Anything new that appears while the watcher runs is queued by notify.
    let mut dirty_since: Option<SystemTime> = None;
    let mut pending: Vec<PathBuf> = Vec::new();

    loop {
        let timeout = if pending.is_empty() {
            Duration::from_secs(60)
        } else {
            Duration::from_millis(900)
        };
        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(path)) => {
                if path.is_dir() || !path.exists() {
                    continue;
                }
                if util::is_junk(&path, &file_name_of(&path)) {
                    continue;
                }
                if !args.only.is_empty() {
                    let ext = util::ext_of(&file_name_of(&path));
                    if !args.only.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                        continue;
                    }
                }
                dirty_since = Some(SystemTime::now());
                if !pending.iter().any(|p| p == &path) {
                    pending.push(path);
                }
            }
            Ok(None) => break,
            Err(_) => {
                // Quiet spell: send whatever has settled.
                if !pending.is_empty() && dirty_since.is_some_and(|t| t.elapsed().unwrap_or_default() > Duration::from_millis(700)) {
                    let batch = std::mem::take(&mut pending);
                    for path in batch {
                        if state.contains(&path) {
                            continue;
                        }
                        let rel = rel_for(&args.paths, &path);
                        match client.send_file(&path, &rel).await {
                            Ok((status, _)) => {
                                if status != "duplicate" {
                                    println!("  new file → {rel}");
                                }
                                state.mark(&path);
                                state.flush();
                            }
                            Err(err) => eprintln!("✗ {rel} — {err:#}"),
                        }
                    }
                    dirty_since = None;
                }
            }
        }
    }
    Ok(())
}

fn rel_for(roots: &[PathBuf], path: &Path) -> String {
    for root in roots {
        if let Ok(stripped) = path.strip_prefix(root) {
            let prefix = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let inner = stripped.to_string_lossy().replace('\\', "/");
            return if prefix.is_empty() {
                inner
            } else {
                format!("{prefix}/{inner}")
            };
        }
    }
    file_name_of(path)
}
