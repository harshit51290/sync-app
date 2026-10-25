use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Turn an arbitrary string from a phone into something safe to use as a
/// single path component on Windows. Blocks traversal, reserved device names,
/// control characters, trailing dots/spaces and characters NTFS rejects.
pub fn sanitize_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let bad = matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            || (ch as u32) < 0x20
            || ch == '\u{7f}';
        out.push(if bad { '_' } else { ch });
    }
    let trimmed = out.trim().trim_end_matches(['.', ' ']);
    let mut clean: String = trimmed.chars().take(120).collect();
    if clean.is_empty() {
        clean = "unnamed".into();
    }
    let stem_upper = clean.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem_upper.as_str()) {
        clean = format!("_{clean}");
    }
    clean
}

/// Split a phone-supplied relative path into safe components.
pub fn sanitize_rel_path(raw: &str) -> PathBuf {
    let mut path = PathBuf::new();
    for part in raw.split(['/', '\\']) {
        let part = part.trim();
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        path.push(sanitize_component(part));
    }
    path
}

/// Android leaves these around everywhere; they are worthless on a laptop.
pub fn is_junk(path: &Path, name: &str) -> bool {
    const JUNK_DIRS: [&str; 10] = [
        ".thumbnails",
        ".trashed",
        ".trash",
        ".pending",
        "Android/data",
        "Android/obb",
        ".cache",
        "__MACOSX",
        ".git",
        "node_modules",
    ];
    if name.starts_with('.') {
        return true;
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    JUNK_DIRS
        .iter()
        .any(|dir| normalized.split('/').any(|c| c == *dir) || normalized.starts_with(dir))
}

pub fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn sha256_blocking(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn local_ip() -> Option<std::net::IpAddr> {
    local_ip_address::local_ip().ok()
}

/// The address a phone on the same Wi-Fi should use.
pub fn lan_ip() -> String {
    match local_ip() {
        Some(ip) if ip.is_ipv4() => ip.to_string(),
        _ => "127.0.0.1".to_string(),
    }
}

pub fn os_open(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

pub fn reveal_in_folder(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(not(windows))]
    {
        if let Some(parent) = path.parent() {
            os_open(parent);
        }
    }
}

#[cfg(windows)]
pub fn hide_console_window() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleWindow() -> *mut c_void;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
    }
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.is_null() {
            ShowWindow(hwnd, 0);
        }
    }
}

#[cfg(not(windows))]
pub fn hide_console_window() {}

/// Hide the console only when this process is its sole owner (a double-click
/// from Explorer). When launched from a terminal the console is shared with the
/// shell, so it must stay visible.
#[cfg(windows)]
pub fn hide_console_if_private() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleWindow() -> *mut c_void;
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn ShowWindow(hwnd: *mut c_void, cmd: i32) -> i32;
    }
    unsafe {
        let mut pids = [0u32; 8];
        let attached = GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32);
        let hwnd = GetConsoleWindow();
        if attached == 1 && !hwnd.is_null() {
            ShowWindow(hwnd, 0);
        }
    }
}

#[cfg(not(windows))]
pub fn hide_console_if_private() {}

/// Free space on the volume that holds `path`, in bytes.
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct UlargeInteger {
            low: u32,
            high: u32,
        }
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(
                dir: *const u16,
                free_to_caller: *mut UlargeInteger,
                total: *mut UlargeInteger,
                total_free: *mut UlargeInteger,
            ) -> i32;
        }
        let wide: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut free = UlargeInteger { low: 0, high: 0 };
        let mut total = UlargeInteger { low: 0, high: 0 };
        let mut total_free = UlargeInteger { low: 0, high: 0 };
        let ok =
            unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) };
        if ok != 0 {
            Some(((free.high as u64) << 32) | free.low as u64)
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_traversal() {
        let p = sanitize_rel_path("../../Windows/System32/evil.exe");
        assert_eq!(p.to_string_lossy(), "Windows\\System32\\evil.exe");
        let p = sanitize_rel_path("..\\..\\..\\boot.ini");
        assert_eq!(p.to_string_lossy(), "boot.ini");
    }

    #[test]
    fn strips_windows_hostile_chars() {
        assert_eq!(sanitize_component("a<b>c:d\"e|f?g*h"), "a_b_c_d_e_f_g_h");
        assert_eq!(sanitize_component("trailing...   "), "trailing");
        assert_eq!(sanitize_component("  "), "unnamed");
        assert_eq!(sanitize_component("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_component("normal.jpg"), "normal.jpg");
    }

    #[test]
    fn junk_filter() {
        assert!(is_junk(
            Path::new("/sdcard/DCIM/.thumbnails/x.jpg"),
            ".thumbnails"
        ));
        assert!(is_junk(Path::new("/sdcard/DCIM/x.jpg"), ".nomedia"));
        assert!(!is_junk(
            Path::new("/sdcard/DCIM/Camera/IMG_1.jpg"),
            "IMG_1.jpg"
        ));
    }

    #[test]
    fn sizes() {
        assert_eq!(human_bytes(999), "999 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
