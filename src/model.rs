use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A directory the user added to Disc. `id` is stable; `name` is editable.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(rename = "folderPath")]
    pub folder_path: String,
}

/// One audio (or video) file found by a scan. Its id is its full path —
/// the same convention the Electron version used, so favorites carry over.
#[derive(Debug)]
pub struct Track {
    pub id: String,
    pub path: PathBuf,
    pub file_name: String,
    /// File name without its extension, for display.
    pub stem: String,
    /// Directory relative to the folder root, "/"-separated; "" for the root.
    pub rel_dir: String,
    pub size: u64,
    pub is_video: bool,
    pub folder_id: String,
}

pub type TrackRef = Arc<Track>;

pub fn folder_name_from_path(p: &str) -> String {
    p.split(['\\', '/'])
        .filter(|s| !s.is_empty())
        .next_back()
        .unwrap_or(p)
        .to_string()
}

pub fn new_folder_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    // Same shape the Electron version used: folder-<ms>-<random>.
    let rand = (now.subsec_nanos() as u64).wrapping_mul(6364136223846793005) >> 40;
    format!("folder-{}-{:05x}", now.as_millis(), rand & 0xfffff)
}

pub fn format_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.0} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}

pub fn format_duration(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return String::new();
    }
    let total = secs.round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
