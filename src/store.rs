use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Organize;
use crate::util::{self, human_bytes};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: String,
    pub name: String,
    /// Path relative to the storage root, with forward slashes.
    pub rel: String,
    pub device: String,
    pub size: u64,
    pub sha256: String,
    /// When the laptop received it (unix ms).
    pub received: i64,
    /// Last-modified time reported by the phone (unix ms).
    pub mtime: i64,
    #[serde(default)]
    pub duplicate: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceRecord {
    pub name: String,
    pub last_seen: i64,
    pub files: u64,
    pub bytes: u64,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub ip: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayBucket {
    pub day: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub files: usize,
    pub bytes: u64,
    pub today_files: usize,
    pub today_bytes: u64,
    pub devices: Vec<DeviceRecord>,
    pub free_space: Option<u64>,
    pub used_by_library: u64,
    pub root: String,
    pub days: Vec<DayBucket>,
    pub human_total: String,
    pub human_free: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    files: Vec<FileRecord>,
    #[serde(default)]
    devices: BTreeMap<String, DeviceRecord>,
}

pub struct NewFile {
    pub name: String,
    pub rel: String,
    pub device: String,
    pub size: u64,
    pub mtime: i64,
    pub sha256: String,
    pub ip: String,
    pub platform: String,
}

pub enum Placed {
    Stored(FileRecord),
    Duplicate(FileRecord),
}

pub struct Store {
    pub root: PathBuf,
    meta_file: PathBuf,
    index: Mutex<Index>,
    hashes: Mutex<HashMap<String, String>>,
}

impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        let meta_file = root.join(".phonesync").join("index.json");
        let index: Index = match std::fs::read(&meta_file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Index::default(),
        };
        let mut hashes = HashMap::new();
        for file in &index.files {
            if !file.duplicate {
                hashes.insert(file.sha256.clone(), file.rel.clone());
            }
        }
        Ok(Self {
            root,
            meta_file,
            index: Mutex::new(index),
            hashes: Mutex::new(hashes),
        })
    }

    fn persist(&self, index: &Index) -> Result<()> {
        if let Some(parent) = self.meta_file.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let tmp = self.meta_file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(index)?)
            .with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.meta_file)
            .with_context(|| format!("replacing {}", self.meta_file.display()))?;
        Ok(())
    }

    /// Move a finished temp file into its final home. Synchronous on purpose:
    /// callers run it inside `spawn_blocking`.
    pub fn place(
        &self,
        tmp: &Path,
        new: NewFile,
        organize: Organize,
        sidecars: bool,
    ) -> Result<Placed> {
        let mut index = self.index.lock().unwrap();
        let mut hashes = self.hashes.lock().unwrap();

        if let Some(existing_rel) = hashes.get(&new.sha256) {
            let existing = index.files.iter().find(|f| &f.rel == existing_rel).cloned();
            if let Some(record) = existing {
                index.files.push(FileRecord {
                    id: format!("d{}", util::now_ms()),
                    duplicate: true,
                    ..record.clone()
                });
                let _ = self.persist(&index);
                let _ = std::fs::remove_file(tmp);
                return Ok(Placed::Duplicate(record));
            }
        }

        let target = self.choose_target(&new, organize)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }

        // Same file already on disk but not in the index (e.g. index was reset).
        if target.exists() {
            if let Ok(existing_hash) = crate::util::sha256_blocking(&target) {
                if existing_hash == new.sha256 {
                    let _ = std::fs::remove_file(tmp);
                    let record = FileRecord {
                        id: format!("f{}", index.files.len() + 1),
                        name: new.name,
                        rel: rel_string(&self.root, &target),
                        device: new.device,
                        size: new.size,
                        sha256: new.sha256,
                        received: util::now_ms(),
                        mtime: new.mtime,
                        duplicate: true,
                    };
                    index.files.push(record.clone());
                    let _ = self.persist(&index);
                    return Ok(Placed::Duplicate(record));
                }
            }
        }

        std::fs::rename(tmp, &target)
            .with_context(|| format!("moving upload into {}", target.display()))?;

        let final_name = target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| new.name.clone());
        let record = FileRecord {
            id: format!("f{}", index.files.len() + 1),
            name: final_name.clone(),
            rel: rel_string(&self.root, &target),
            device: new.device.clone(),
            size: new.size,
            sha256: new.sha256.clone(),
            received: util::now_ms(),
            mtime: new.mtime,
            duplicate: false,
        };

        if sidecars {
            let sidecar = target.with_extension(format!(
                "{}.phonesync.json",
                target
                    .extension()
                    .map(|e| e.to_string_lossy().to_string())
                    .unwrap_or_default()
            ));
            let payload = serde_json::json!({
                "device": new.device,
                "original_name": new.name,
                "sha256": new.sha256,
                "size": new.size,
                "source_ip": new.ip,
                "platform": new.platform,
                "mtime_ms": new.mtime,
                "received_ms": record.received,
            });
            let _ = std::fs::write(sidecar, serde_json::to_vec_pretty(&payload)?);
        }

        hashes.insert(new.sha256, record.rel.clone());
        index.files.push(record.clone());

        let device = index
            .devices
            .entry(record.device.clone())
            .or_insert_with(|| DeviceRecord {
                name: record.device.clone(),
                ..Default::default()
            });
        device.last_seen = record.received;
        device.files += 1;
        device.bytes += record.size;
        if !new.ip.is_empty() {
            device.ip = new.ip;
        }
        if !new.platform.is_empty() {
            device.platform = new.platform;
        }

        self.persist(&index)?;
        Ok(Placed::Stored(record))
    }

    fn choose_target(&self, new: &NewFile, organize: Organize) -> Result<PathBuf> {
        let device_dir = self.root.join(util::sanitize_component(&new.device));
        let rel = util::sanitize_rel_path(&new.rel);
        let name = util::sanitize_component(&new.name);

        let candidate = match organize {
            Organize::Mirror if !rel.as_os_str().is_empty() => device_dir.join(&rel),
            Organize::Flat => device_dir.join(&name),
            _ => device_dir.join(month_of(new.mtime)).join(&name),
        };

        if !candidate.exists() {
            return Ok(candidate);
        }
        // Name collision with different content: add " (2)", " (3)", ...
        let stem = candidate
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let ext = candidate
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let parent = candidate.parent().unwrap_or(Path::new("."));
        for n in 2..10_000 {
            let next = parent.join(format!("{stem} ({n}){ext}"));
            if !next.exists() {
                return Ok(next);
            }
        }
        anyhow::bail!("could not find a free filename for {}", name)
    }

    pub fn record_device(&self, name: &str, ip: &str, platform: &str) {
        let mut index = self.index.lock().unwrap();
        let device = index
            .devices
            .entry(name.to_string())
            .or_insert_with(|| DeviceRecord {
                name: name.to_string(),
                ..Default::default()
            });
        device.last_seen = util::now_ms();
        if !ip.is_empty() {
            device.ip = ip.to_string();
        }
        if !platform.is_empty() {
            device.platform = platform.to_string();
        }
        let _ = self.persist(&index);
    }

    pub fn devices(&self) -> Vec<DeviceRecord> {
        let index = self.index.lock().unwrap();
        let mut list: Vec<_> = index.devices.values().cloned().collect();
        list.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        list
    }

    pub fn files(&self, limit: usize, query: &str) -> Vec<FileRecord> {
        let index = self.index.lock().unwrap();
        let needle = query.to_ascii_lowercase();
        index
            .files
            .iter()
            .rev()
            .filter(|f| !f.duplicate)
            .filter(|f| {
                needle.is_empty()
                    || f.name.to_ascii_lowercase().contains(&needle)
                    || f.device.to_ascii_lowercase().contains(&needle)
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn find(&self, id: &str) -> Option<FileRecord> {
        let index = self.index.lock().unwrap();
        index.files.iter().find(|f| f.id == id).cloned()
    }

    pub fn abs_path(&self, record: &FileRecord) -> PathBuf {
        self.root
            .join(record.rel.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    pub fn stats(&self) -> Stats {
        let index = self.index.lock().unwrap();
        let now = chrono::Local::now();
        let today = now.format("%Y-%m-%d").to_string();

        let mut days: BTreeMap<String, DayBucket> = BTreeMap::new();
        for offset in (0..14).rev() {
            let day = (now - chrono::Duration::days(offset))
                .format("%Y-%m-%d")
                .to_string();
            days.insert(
                day.clone(),
                DayBucket {
                    day,
                    files: 0,
                    bytes: 0,
                },
            );
        }

        let mut bytes = 0u64;
        let mut today_files = 0;
        let mut today_bytes = 0u64;
        let mut count = 0usize;
        for file in index.files.iter().filter(|f| !f.duplicate) {
            count += 1;
            bytes += file.size;
            let day = chrono::DateTime::from_timestamp_millis(file.received)
                .map(|dt| {
                    dt.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d")
                        .to_string()
                })
                .unwrap_or_default();
            if day == today {
                today_files += 1;
                today_bytes += file.size;
            }
            if let Some(bucket) = days.get_mut(&day) {
                bucket.files += 1;
                bucket.bytes += file.size;
            }
        }

        let free = util::free_space(&self.root);
        let mut devices: Vec<_> = index.devices.values().cloned().collect();
        devices.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

        Stats {
            files: count,
            bytes,
            today_files,
            today_bytes,
            devices,
            free_space: free,
            used_by_library: bytes,
            root: self.root.display().to_string(),
            days: days.into_values().collect(),
            human_total: human_bytes(bytes),
            human_free: free.map(human_bytes).unwrap_or_else(|| "?".into()),
        }
    }
}

fn month_of(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m").to_string())
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m").to_string())
}

fn rel_string(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn newfile(name: &str, content: &str) -> NewFile {
        NewFile {
            name: name.to_string(),
            rel: String::new(),
            device: "Pixel".into(),
            size: content.len() as u64,
            mtime: 1_760_000_000_000,
            sha256: util::sha256_bytes(content.as_bytes()),
            ip: "192.168.1.5".into(),
            platform: "android".into(),
        }
    }

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        (dir, store)
    }

    #[test]
    fn stores_and_dedupes() {
        let (dir, store) = temp_store();
        let tmp = dir.path().join("a.tmp");
        std::fs::write(&tmp, "hello").unwrap();
        let placed = store
            .place(&tmp, newfile("IMG_1.jpg", "hello"), Organize::Month, false)
            .unwrap();
        let record = match placed {
            Placed::Stored(r) => r,
            Placed::Duplicate(_) => panic!("first upload must be stored"),
        };
        assert!(record.rel.contains("Pixel"));
        assert!(store.abs_path(&record).exists());

        let tmp2 = dir.path().join("b.tmp");
        std::fs::write(&tmp2, "hello").unwrap();
        let placed = store
            .place(&tmp2, newfile("IMG_1.jpg", "hello"), Organize::Month, false)
            .unwrap();
        assert!(matches!(placed, Placed::Duplicate(_)));
        assert!(!tmp2.exists(), "duplicate temp file is removed");
        assert_eq!(store.stats().files, 1);
    }

    #[test]
    fn collisions_get_suffix() {
        let (dir, store) = temp_store();
        for (i, body) in ["one", "two"].iter().enumerate() {
            let tmp = dir.path().join(format!("{i}.tmp"));
            std::fs::write(&tmp, body).unwrap();
            let mut nf = newfile("same.jpg", body);
            nf.sha256 = util::sha256_bytes(body.as_bytes());
            store.place(&tmp, nf, Organize::Flat, false).unwrap();
        }
        let files = store.files(10, "");
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.name == "same (2).jpg"));
    }

    #[test]
    fn search_and_stats() {
        let (dir, store) = temp_store();
        let tmp = dir.path().join("x.tmp");
        std::fs::write(&tmp, "abc").unwrap();
        store
            .place(&tmp, newfile("holiday.png", "abc"), Organize::Month, false)
            .unwrap();
        assert_eq!(store.files(10, "holi").len(), 1);
        assert_eq!(store.files(10, "nope").len(), 0);
        let stats = store.stats();
        assert_eq!(stats.files, 1);
        assert_eq!(stats.bytes, 3);
    }
}
