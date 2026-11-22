//! The app side of the Claude connector. A running Disc Lite listens on a
//! local-only port (127.0.0.1, random, with a random token) and writes how
//! to reach it to `connector.json` in its data folder. `disc-lite --mcp`
//! (mcp.rs — the process Claude starts) talks to it there.
//!
//! Requests are answered on the bridge's own threads from a snapshot the
//! app publishes whenever its library changes, so Claude gets an answer even
//! while Disc is minimized or busy. Renames happen here too; the app is then
//! told about them (to carry favorites, removed tracks, the queue and
//! waveforms over to the new names, and to make them undoable).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use serde_json::{Value, json};

use crate::model::{Folder, TrackRef};
use crate::tree::{Model, NodeKind};

/// What Claude can see: the app's library as of its last change.
#[derive(Default)]
pub struct Snapshot {
    pub folders: Vec<Folder>,
    pub model: Arc<Model>,
    pub all: Vec<TrackRef>,
    /// Exactly what the track list shows right now (search applied).
    pub view: Vec<TrackRef>,
    /// "all" | "favorites" | "removed" | a tree node key
    pub active: String,
    pub active_name: String,
    pub search: String,
    /// Some folders haven't finished their first scan yet.
    pub scanning: bool,
}

pub enum Event {
    /// (old path, new path) for each track Claude renamed.
    Renamed(Vec<(PathBuf, PathBuf)>),
}

struct Shared {
    snap: Arc<Snapshot>,
    /// Renamed by the bridge but not yet reflected in a snapshot.
    renamed: HashMap<String, PathBuf>,
}

pub struct Bridge {
    shared: Arc<RwLock<Shared>>,
    pub events: Receiver<Event>,
    info_path: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Info {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

pub fn info_path() -> PathBuf {
    crate::persist::data_dir().join("connector.json")
}

fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        // RandomState is seeded from the OS's random source.
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i ^ std::process::id() as u64);
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

impl Bridge {
    pub fn start(ctx: egui::Context) -> Option<Bridge> {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        let token = random_token();
        let info = Info { port, token: token.clone(), pid: std::process::id() };
        let info_path = info_path();
        if let Some(dir) = info_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        std::fs::write(&info_path, serde_json::to_vec(&info).ok()?).ok()?;

        let shared = Arc::new(RwLock::new(Shared { snap: Arc::new(Snapshot::default()), renamed: HashMap::new() }));
        let (tx, rx) = unbounded();
        let sh = shared.clone();
        std::thread::Builder::new()
            .name("connector".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let (sh, tx, ctx, token) = (sh.clone(), tx.clone(), ctx.clone(), token.clone());
                    std::thread::spawn(move || serve(stream, &token, &sh, &tx, &ctx));
                }
            })
            .ok()?;
        Some(Bridge { shared, events: rx, info_path })
    }

    pub fn publish(&self, snap: Snapshot) {
        let mut sh = self.shared.write().unwrap();
        // Keep pending renames only while the snapshot still has the old path.
        sh.renamed.retain(|old, _| snap.all.iter().any(|t| &t.id == old));
        sh.snap = Arc::new(snap);
    }

    /// Stop advertising this instance (only if connector.json is still ours).
    pub fn shutdown(&self) {
        if let Ok(text) = std::fs::read_to_string(&self.info_path) {
            if serde_json::from_str::<Info>(&text).is_ok_and(|i| i.pid == std::process::id()) {
                let _ = std::fs::remove_file(&self.info_path);
            }
        }
    }
}

fn serve(stream: TcpStream, token: &str, shared: &RwLock<Shared>, tx: &Sender<Event>, ctx: &egui::Context) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let Ok(mut writer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(_) => json!({ "error": "bad request" }),
            Ok(req) if req["token"].as_str() != Some(token) => {
                let _ = writeln!(writer, "{}", json!({ "error": "bad token" }));
                return;
            }
            Ok(req) => match handle(req["method"].as_str().unwrap_or(""), &req["params"], shared, tx, ctx) {
                Ok(v) => json!({ "ok": v }),
                Err(e) => json!({ "error": e }),
            },
        };
        if writeln!(writer, "{reply}").is_err() {
            return;
        }
        line.clear();
    }
}

fn handle(method: &str, params: &Value, shared: &RwLock<Shared>, tx: &Sender<Event>, ctx: &egui::Context) -> Result<Value, String> {
    let (snap, renamed) = {
        let sh = shared.read().unwrap();
        (sh.snap.clone(), sh.renamed.clone())
    };
    match method {
        "status" => Ok(json!({ "scanning": snap.scanning, "version": crate::update::VERSION })),
        "list_folders" => Ok(list_folders(&snap)),
        "list_tracks" => list_tracks(&snap, &renamed, params),
        "rename_tracks" => {
            let (result, done) = rename_tracks(&snap, &renamed, params)?;
            if !done.is_empty() {
                {
                    let mut sh = shared.write().unwrap();
                    for (old, new) in &done {
                        sh.renamed.insert(old.to_string_lossy().into_owned(), new.clone());
                    }
                }
                let _ = tx.send(Event::Renamed(done));
                ctx.request_repaint();
            }
            Ok(result)
        }
        _ => Err(format!("unknown method {method}")),
    }
}

// --- Folders ---------------------------------------------------------------

/// Every node in tree order: (key, depth).
fn tree_order(model: &Model) -> Vec<(String, usize)> {
    fn walk(m: &Model, parent: &str, depth: usize, out: &mut Vec<(String, usize)>) {
        for k in m.children(parent) {
            out.push((k.clone(), depth));
            walk(m, k, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(model, "", 0, &mut out);
    out
}

fn node_path(snap: &Snapshot, key: &str) -> Option<PathBuf> {
    let (folder_id, sub) = crate::tree::parse_selection(key);
    let f = snap.folders.iter().find(|f| f.id == folder_id)?;
    let mut dir = PathBuf::from(&f.folder_path);
    for part in sub.split('/').filter(|s| !s.is_empty()) {
        dir.push(part);
    }
    Some(dir)
}

fn list_folders(snap: &Snapshot) -> Value {
    let m = &snap.model;
    let folders: Vec<Value> = tree_order(m)
        .into_iter()
        .filter_map(|(key, depth)| {
            let n = m.nodes.get(&key)?;
            let parent = m.eff_parent.get(&key).cloned().unwrap_or_default();
            Some(json!({
                "key": key,
                "name": n.name,
                "path": node_path(snap, &key).map(|p| p.to_string_lossy().into_owned()),
                "parent_key": if parent.is_empty() { Value::Null } else { Value::String(parent) },
                "depth": depth,
                "kind": if n.kind == NodeKind::Folder { "added folder" } else { "subfolder" },
                "track_count": m.node_tracks(&key).len(),
            }))
        })
        .collect();
    json!({
        "showing_in_disc": { "key": snap.active, "name": snap.active_name, "search": snap.search, "track_count": snap.view.len() },
        "library_track_count": snap.all.len(),
        "still_scanning": snap.scanning,
        "folders": folders,
    })
}

/// Finds the tracks a `folder` argument means. None/"" = what Disc shows.
fn resolve<'a>(snap: &'a Snapshot, folder: Option<&str>) -> Result<(String, String, Vec<TrackRef>), String> {
    let folder = folder.map(str::trim).unwrap_or("");
    let lower = folder.to_lowercase();
    match lower.as_str() {
        "" | "current" | "showing" => return Ok((snap.active.clone(), snap.active_name.clone(), snap.view.clone())),
        "all" | "all music" => return Ok(("all".into(), "All Music".into(), snap.all.clone())),
        "favorites" => {
            return Err("Favorites can't be listed through the connector yet; use a folder or All Music.".into());
        }
        _ => {}
    }
    let m = &snap.model;
    if m.nodes.contains_key(folder) {
        let name = m.nodes[folder].name.clone();
        return Ok((folder.to_string(), name, m.node_tracks(folder)));
    }
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let matches: Vec<String> = tree_order(m)
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| {
            m.nodes[k].name.to_lowercase() == lower || node_path(snap, k).is_some_and(|p| norm(&p.to_string_lossy()) == norm(folder))
        })
        .collect();
    match matches.len() {
        0 => Err(format!("No folder called \"{folder}\" in Disc. Use list_folders to see them.")),
        1 => {
            let k = &matches[0];
            Ok((k.clone(), m.nodes[k].name.clone(), m.node_tracks(k)))
        }
        _ => {
            let options: Vec<String> = matches
                .iter()
                .map(|k| format!("{} ({})", k, node_path(snap, k).map(|p| p.display().to_string()).unwrap_or_default()))
                .collect();
            Err(format!("Several folders are called \"{folder}\"; pass one of these keys instead: {}", options.join(", ")))
        }
    }
}

fn current_path(t: &TrackRef, renamed: &HashMap<String, PathBuf>) -> PathBuf {
    renamed.get(&t.id).cloned().unwrap_or_else(|| t.path.clone())
}

fn list_tracks(snap: &Snapshot, renamed: &HashMap<String, PathBuf>, params: &Value) -> Result<Value, String> {
    let (key, name, mut tracks) = resolve(snap, params["folder"].as_str())?;
    if let Some(q) = params["search"].as_str().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()) {
        tracks.retain(|t| t.file_name.to_lowercase().contains(&q));
    }
    let total = tracks.len();
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let limit = params["limit"].as_u64().unwrap_or(200).clamp(1, 1000) as usize;
    let items: Vec<Value> = tracks
        .iter()
        .skip(offset)
        .take(limit)
        .map(|t| {
            let p = current_path(t, renamed);
            json!({
                "path": p.to_string_lossy(),
                "name": p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                "extension": p.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                "subfolder": t.rel_dir,
            })
        })
        .collect();
    Ok(json!({
        "folder": { "key": key, "name": name },
        "total": total,
        "offset": offset,
        "returned": items.len(),
        "still_scanning": snap.scanning,
        "tracks": items,
    }))
}

// --- Renaming -----------------------------------------------------------------

fn same_path(a: &Path, b: &Path) -> bool {
    let n = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    n(a) == n(b)
}

/// The new file name (without extension) Claude asked for, tidied: an
/// extension it repeated is dropped and runs of spaces are collapsed.
pub fn clean_name(new_name: &str, ext: &str) -> String {
    let mut s = new_name.trim().to_string();
    if !ext.is_empty() && s.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) {
        s.truncate(s.len() - ext.len() - 1);
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // Windows doesn't allow trailing dots or spaces.
    s.trim_end_matches(['.', ' ']).to_string()
}

fn rename_tracks(snap: &Snapshot, renamed: &HashMap<String, PathBuf>, params: &Value) -> Result<(Value, Vec<(PathBuf, PathBuf)>), String> {
    let Some(list) = params["renames"].as_array() else {
        return Err("renames must be a list of { path, new_name }".into());
    };
    let known: Vec<PathBuf> = snap.all.iter().map(|t| current_path(t, renamed)).collect();
    let mut results = Vec::new();
    let mut done = Vec::new();
    for item in list {
        let path = item["path"].as_str().unwrap_or("");
        let new_name = item["new_name"].as_str().unwrap_or("");
        // Use the path exactly as Disc knows it (Claude may change its case).
        let found = known.iter().find(|k| same_path(k, Path::new(path))).cloned();
        let p = found.clone().unwrap_or_else(|| PathBuf::from(path));
        let outcome = if path.is_empty() || new_name.trim().is_empty() {
            Err("needs both path and new_name".to_string())
        } else if found.is_none() {
            Err("not a track in Disc (use a path from list_tracks)".to_string())
        } else {
            let ext = p.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
            crate::library::rename_track(&p, &clean_name(new_name, &ext))
        };
        match outcome {
            Ok(new_path) => {
                let changed = new_path.to_string_lossy() != p.to_string_lossy();
                results.push(json!({ "path": path, "new_path": new_path.to_string_lossy(), "changed": changed }));
                if changed {
                    done.push((p, new_path));
                }
            }
            Err(e) => results.push(json!({ "path": path, "error": e })),
        }
    }
    let failed = results.iter().filter(|r| r.get("error").is_some()).count();
    Ok((json!({ "renamed": done.len(), "failed": failed, "results": results, "undo": "Ctrl+Z in Disc Lite undoes this whole batch" }), done))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_name_tidies_what_claude_sends() {
        assert_eq!(clean_name("  Song  Title -  Artist ", "mp3"), "Song Title - Artist");
        assert_eq!(clean_name("Song.MP3", "mp3"), "Song");
        assert_eq!(clean_name("Mr. Blue Sky.", "wav"), "Mr. Blue Sky");
        assert_eq!(clean_name("v1.2", "mp3"), "v1.2");
    }
}
