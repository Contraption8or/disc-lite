//! Folder scanning and live watching, all off the UI thread. Results come
//! back over a channel; the UI thread drains it each frame.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::model::{Track, TrackRef};

/// Directories an older Disc may have created that should never show up as
/// library content.
const SKIP_DIRS: [&str; 2] = [".disc-sections", ".disc-sync"];

pub enum ScanResult {
    /// None = the folder itself couldn't be reached (unplugged drive, etc).
    Scanned { folder_id: String, tracks: Option<Vec<TrackRef>> },
}

fn file_kind(name: &str) -> Option<bool> {
    // Some(is_video)
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".mov") {
        Some(true)
    } else if lower.ends_with(".mp3") || lower.ends_with(".wav") || lower.ends_with(".ogg") {
        Some(false)
    } else {
        None
    }
}

pub fn is_importable(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| file_kind(n).is_some())
}

/// Walks a folder for audio/video files. None if the root doesn't exist.
pub fn scan_folder(folder_id: &str, root: &Path) -> Option<Vec<TrackRef>> {
    if !root.is_dir() {
        return None;
    }
    let mut out = Vec::new();
    let walker = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && e.file_name().to_str().is_some_and(|n| SKIP_DIRS.contains(&n)))
        });
    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else { continue };
        let Some(is_video) = file_kind(name) else { continue };
        let path = entry.path().to_path_buf();
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let rel_dir = path
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| {
                p.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        let stem = match name.rfind('.') {
            Some(i) if i > 0 => name[..i].to_string(),
            _ => name.to_string(),
        };
        out.push(Arc::new(Track {
            id: path.to_string_lossy().into_owned(),
            path,
            file_name: name.to_string(),
            stem,
            rel_dir,
            size,
            is_video,
            folder_id: folder_id.to_string(),
        }));
    }
    Some(out)
}

/// Owns the background scanner and the file watcher.
pub struct Library {
    scan_tx: Sender<(String, PathBuf)>,
    pub results: Receiver<ScanResult>,
    watcher: Option<RecommendedWatcher>,
    /// root path -> folder id, shared with the watcher callback.
    roots: Arc<Mutex<Vec<(PathBuf, String)>>>,
    watched: HashMap<String, PathBuf>,
}

impl Library {
    pub fn new(ctx: egui::Context) -> Self {
        let (scan_tx, scan_rx) = unbounded::<(String, PathBuf)>();
        let (result_tx, results) = unbounded();

        // Scanner pool: a few threads pulling scan requests, so many folders
        // scan in parallel at startup.
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
        for _ in 0..threads {
            let rx = scan_rx.clone();
            let tx = result_tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                while let Ok((folder_id, root)) = rx.recv() {
                    let tracks = scan_folder(&folder_id, &root);
                    let _ = tx.send(ScanResult::Scanned { folder_id, tracks });
                    ctx.request_repaint();
                }
            });
        }

        // Watcher -> debounce -> rescan. Filesystem events arrive in bursts
        // (a copy of 50 files is hundreds of events), so a folder is only
        // rescanned once things have been quiet for a moment.
        let roots: Arc<Mutex<Vec<(PathBuf, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let (changed_tx, changed_rx) = unbounded::<String>();
        let roots_for_cb = roots.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, notify::EventKind::Access(_)) {
                return;
            }
            let roots = roots_for_cb.lock().unwrap();
            for p in &event.paths {
                for (root, id) in roots.iter() {
                    if p.starts_with(root) {
                        let _ = changed_tx.send(id.clone());
                    }
                }
            }
        })
        .ok();

        let rescan_tx = scan_tx.clone();
        let roots_for_debounce = roots.clone();
        std::thread::spawn(move || {
            let quiet = Duration::from_millis(400);
            let mut pending: HashMap<String, Instant> = HashMap::new();
            loop {
                let timeout = pending
                    .values()
                    .map(|t| (*t + quiet).saturating_duration_since(Instant::now()))
                    .min()
                    .unwrap_or(Duration::from_secs(3600));
                match changed_rx.recv_timeout(timeout) {
                    Ok(id) => {
                        pending.insert(id, Instant::now());
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(_) => return,
                }
                let now = Instant::now();
                let due: Vec<String> = pending
                    .iter()
                    .filter(|(_, t)| now.duration_since(**t) >= quiet)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in due {
                    pending.remove(&id);
                    let root = roots_for_debounce
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|(_, i)| *i == id)
                        .map(|(r, _)| r.clone());
                    if let Some(root) = root {
                        let _ = rescan_tx.send((id, root));
                    }
                }
            }
        });

        Self { scan_tx, results, watcher, roots, watched: HashMap::new() }
    }

    /// For background work (file copies) that wants a rescan when it's done.
    pub fn scan_sender(&self) -> Sender<(String, PathBuf)> {
        self.scan_tx.clone()
    }

    pub fn scan(&self, folder_id: &str, root: &str) {
        let _ = self.scan_tx.send((folder_id.to_string(), PathBuf::from(root)));
    }

    /// Makes the watched set match `folders`: watch new ones, stop watching
    /// removed ones. Returns the ids that are newly watched (so they get an
    /// initial scan).
    pub fn sync_watches(&mut self, folders: &[(String, String)]) -> Vec<String> {
        let mut added = Vec::new();
        let wanted: HashMap<&String, &String> = folders.iter().map(|(id, p)| (id, p)).collect();
        let gone: Vec<String> = self
            .watched
            .keys()
            .filter(|id| !wanted.contains_key(id))
            .cloned()
            .collect();
        for id in gone {
            if let Some(path) = self.watched.remove(&id) {
                if let Some(w) = self.watcher.as_mut() {
                    let _ = w.unwatch(&path);
                }
            }
        }
        for (id, path) in folders {
            if self.watched.contains_key(id) {
                continue;
            }
            let p = PathBuf::from(path);
            if let Some(w) = self.watcher.as_mut() {
                let _ = w.watch(&p, RecursiveMode::Recursive);
            }
            self.watched.insert(id.clone(), p);
            added.push(id.clone());
        }
        *self.roots.lock().unwrap() = self.watched.iter().map(|(id, p)| (p.clone(), id.clone())).collect();
        added
    }
}

// --- File operations ------------------------------------------------------

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// Copies files into `dest_dir`. Dropping a file back into the folder it's
/// already in is skipped; a name collision gets " (1)", " (2)", ... like
/// Explorer does. Returns how many were copied.
pub fn copy_into(dest_dir: &Path, sources: &[PathBuf]) -> usize {
    let _ = std::fs::create_dir_all(dest_dir);
    let mut copied = 0;
    for src in sources {
        let Some(base) = src.file_name() else { continue };
        let mut target = dest_dir.join(base);
        if same_path(src, &target) {
            continue;
        }
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = src.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let mut n = 1;
        while target.exists() {
            target = dest_dir.join(format!("{stem} ({n}){ext}"));
            n += 1;
        }
        if std::fs::copy(src, &target).is_ok() {
            copied += 1;
        }
    }
    copied
}

/// Renames a track's file, keeping its extension. Returns the new path.
pub fn rename_track(path: &Path, new_stem: &str) -> Result<PathBuf, String> {
    let new_stem = new_stem.trim();
    if new_stem.is_empty() || new_stem.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|']) {
        return Err("That name has characters Windows doesn't allow.".into());
    }
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let target = path.with_file_name(format!("{new_stem}{ext}"));
    if same_path(&target, path) {
        return Ok(target);
    }
    if target.exists() {
        return Err("A file with that name already exists.".into());
    }
    std::fs::rename(path, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// To the Recycle Bin, never a permanent delete.
pub fn delete_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

pub fn reveal_in_explorer(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn();
    }
}
