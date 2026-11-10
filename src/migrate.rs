//! One-time import of the Electron Disc Lite's setup (folders, how they're
//! arranged, favorites, volume, shuffle). Chromium keeps a page's
//! localStorage in a LevelDB database under the app's data folder; this
//! copies that database somewhere temporary (never touching the original,
//! and working even if the old app is open) and reads the keys we need.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusty_leveldb::{DB, LdbIterator, Options};

use crate::model::Folder;
use crate::persist::Settings;

fn candidate_dirs() -> Vec<PathBuf> {
    let Some(base) = dirs::config_dir() else { return Vec::new() };
    // Packaged builds used "Disc Lite"; running from source used "disc-lite".
    ["Disc Lite", "disc-lite"]
        .iter()
        .map(|name| base.join(name).join("Local Storage").join("leveldb"))
        .filter(|p| p.join("CURRENT").exists())
        .collect()
}

/// Chromium stores each value with a leading encoding byte: 1 = Latin-1,
/// 0 = UTF-16LE.
fn decode_value(raw: &[u8]) -> Option<String> {
    let (&tag, rest) = raw.split_first()?;
    match tag {
        1 => Some(rest.iter().map(|&b| b as char).collect()),
        0 => {
            let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16(&units).ok()
        }
        _ => None,
    }
}

fn copy_db(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "LOCK" || !entry.file_type()?.is_file() {
            continue;
        }
        std::fs::copy(entry.path(), dst.join(name))?;
    }
    Ok(())
}

fn read_local_storage(dir: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let tmp = std::env::temp_dir().join(format!("disc-lite-import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if copy_db(dir, &tmp).is_err() {
        return out;
    }
    if let Ok(mut db) = DB::open(&tmp, Options::default()) {
        if let Ok(mut it) = db.new_iter() {
            while it.advance() {
                let Some((k, v)) = it.current() else { continue };
                let k: &[u8] = k.as_ref();
                // Key layout: "_" + origin + "\0" + "\x01" + key
                let Some(nul) = k.iter().position(|&b| b == 0) else { continue };
                if k.first() != Some(&b'_') || k.get(nul + 1) != Some(&1) {
                    continue;
                }
                let key: String = k[nul + 2..].iter().map(|&b| b as char).collect();
                if let Some(value) = decode_value(v.as_ref()) {
                    out.insert(key, value);
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    out
}

/// Returns the old app's setup if there is one with at least one folder.
pub fn import_from_electron() -> Option<Settings> {
    for dir in candidate_dirs() {
        let ls = read_local_storage(&dir);
        let folders: Vec<Folder> = ls
            .get("lite.folders")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        if folders.is_empty() {
            continue;
        }
        let mut s = Settings {
            folders,
            ..Settings::default()
        };
        if let Some(v) = ls.get("disc.favorites").and_then(|s| serde_json::from_str(s).ok()) {
            s.favorites = v;
        }
        if let Some(v) = ls.get("lite.placement").and_then(|s| serde_json::from_str(s).ok()) {
            s.placement = v;
        }
        if let Some(v) = ls.get("lite.order").and_then(|s| serde_json::from_str(s).ok()) {
            s.order = v;
        }
        if let Some(v) = ls.get("lite.expanded").and_then(|s| serde_json::from_str(s).ok()) {
            s.expanded = v;
        }
        if let Some(v) = ls.get("disc.volume").and_then(|s| s.parse::<f32>().ok()) {
            s.volume = v.clamp(0.0, 1.0);
        }
        s.shuffle = ls.get("disc.shuffle").is_some_and(|v| v == "true");
        return Some(s);
    }
    None
}
