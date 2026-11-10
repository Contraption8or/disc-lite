use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::model::Folder;

fn default_volume() -> f32 {
    0.8
}

/// Everything Disc Lite remembers between launches.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub folders: Vec<Folder>,
    pub favorites: Vec<String>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    pub shuffle: bool,
    pub expanded: Vec<String>,
    pub placement: HashMap<String, String>,
    pub order: HashMap<String, Vec<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            favorites: Vec::new(),
            volume: default_volume(),
            shuffle: false,
            expanded: Vec::new(),
            placement: HashMap::new(),
            order: HashMap::new(),
        }
    }
}

/// %APPDATA%\Disc Lite — the same folder the Electron build used, so the
/// two live side by side (this file is new; nothing of the old app's is
/// modified).
pub fn data_dir() -> PathBuf {
    // Override for testing against a throwaway profile.
    if let Some(dir) = std::env::var_os("DISC_LITE_DATA") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Disc Lite")
}

fn settings_path() -> PathBuf {
    data_dir().join("disc-lite-settings.json")
}

pub fn waveform_cache_path() -> PathBuf {
    data_dir().join("waveforms.cache")
}

/// None means "first launch" (no settings file yet).
pub fn load() -> Option<Settings> {
    let text = std::fs::read_to_string(settings_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// Written to a temp file then renamed, so a crash mid-write can't leave a
/// half-written settings file behind.
pub fn save(settings: &Settings) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let Ok(json) = serde_json::to_string_pretty(settings) else { return };
    let tmp = dir.join("disc-lite-settings.json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, settings_path());
    }
}
