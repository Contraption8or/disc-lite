//! "Connect to Claude": adds (or removes) Disc Lite as an MCP server in the
//! Claude desktop app's config and in Claude Code's user config, pointing
//! at this exe with `--mcp`. Only our own `disc-lite` entry is touched;
//! everything else in those files is kept as it was, in the same order.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

pub const SERVER_NAME: &str = "disc-lite";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum App {
    /// Claude desktop app (chat): claude_desktop_config.json
    Desktop,
    /// Claude Code (CLI, and the desktop app's Code tab): ~/.claude.json
    Code,
}

impl App {
    pub fn label(self) -> &'static str {
        match self {
            App::Desktop => "Claude app",
            App::Code => "Claude Code",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum State {
    Connected,
    /// Set up, but for a Disc Lite exe somewhere else.
    Outdated,
    NotConnected,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub app: App,
    pub path: PathBuf,
    pub state: State,
}

/// Where the Claude desktop app keeps its config. The Microsoft Store
/// build reads it from its package folder.
fn desktop_config() -> Option<PathBuf> {
    if let Some(local) = dirs::data_local_dir() {
        if let Ok(rd) = std::fs::read_dir(local.join("Packages")) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("Claude_") {
                    let dir = e.path().join("LocalCache").join("Roaming").join("Claude");
                    if dir.is_dir() {
                        return Some(dir.join("claude_desktop_config.json"));
                    }
                }
            }
        }
    }
    let dir = dirs::config_dir()?.join("Claude");
    dir.is_dir().then(|| dir.join("claude_desktop_config.json"))
}

/// Claude Code's user config — only if Claude Code has been used here.
fn code_config() -> Option<PathBuf> {
    let p = dirs::home_dir()?.join(".claude.json");
    p.is_file().then_some(p)
}

fn exe() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

fn entry(app: App, exe: &str) -> Value {
    match app {
        App::Desktop => json!({ "command": exe, "args": ["--mcp"] }),
        App::Code => json!({ "type": "stdio", "command": exe, "args": ["--mcp"], "env": {} }),
    }
}

fn read_json(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
            Ok(Value::Object(m)) => Ok(m),
            _ => Err(format!("{} isn't valid JSON, so it was left alone", path.display())),
        },
    }
}

fn state_in(cfg: &Map<String, Value>, exe: &str) -> State {
    match cfg.get("mcpServers").and_then(|s| s.get(SERVER_NAME)) {
        None => State::NotConnected,
        Some(e) if e.get("command").and_then(Value::as_str).is_some_and(|c| c.eq_ignore_ascii_case(exe)) => State::Connected,
        Some(_) => State::Outdated,
    }
}

/// The Claude apps found on this computer, and whether each has Disc Lite.
pub fn targets() -> Vec<Target> {
    let exe = exe();
    [(App::Desktop, desktop_config()), (App::Code, code_config())]
        .into_iter()
        .filter_map(|(app, path)| {
            let path = path?;
            let state = read_json(&path).map(|c| state_in(&c, &exe)).unwrap_or(State::NotConnected);
            Some(Target { app, path, state })
        })
        .collect()
}

/// Writes (connect = true) or removes our entry in one config file.
pub fn set(app: App, path: &Path, connect: bool) -> Result<(), String> {
    let mut cfg = read_json(path)?;
    // Claude Code keeps a lot more than MCP servers in this file: keep a
    // copy of how it was before Disc Lite first touched it.
    if path.is_file() {
        let backup = path.with_extension("json.before-disc-lite");
        if !backup.exists() {
            let _ = std::fs::copy(path, backup);
        }
    }
    apply(&mut cfg, app, &exe(), connect);
    write_atomic(path, &cfg)
}

fn apply(cfg: &mut Map<String, Value>, app: App, exe: &str, connect: bool) {
    let servers = cfg.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
    if !servers.is_object() {
        *servers = Value::Object(Map::new());
    }
    let servers = servers.as_object_mut().unwrap();
    if connect {
        servers.insert(SERVER_NAME.into(), entry(app, exe));
    } else {
        servers.remove(SERVER_NAME);
    }
}

fn write_atomic(path: &Path, cfg: &Map<String, Value>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.disc-lite-tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_and_removes_only_our_entry() {
        let dir = std::env::temp_dir().join(format!("disc-lite-claude-setup-{}", std::process::id()));
        let path = dir.join("cfg.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"zeta":1,"mcpServers":{"other":{"command":"x"}},"alpha":[1,2]}"#).unwrap();

        let mut cfg = read_json(&path).unwrap();
        assert_eq!(state_in(&cfg, r"C:\Apps\disc-lite.exe"), State::NotConnected);
        apply(&mut cfg, App::Code, r"C:\Apps\disc-lite.exe", true);
        write_atomic(&path, &cfg).unwrap();

        let cfg = read_json(&path).unwrap();
        assert_eq!(cfg.keys().collect::<Vec<_>>(), ["zeta", "mcpServers", "alpha"]);
        assert_eq!(cfg["mcpServers"]["other"]["command"], "x");
        assert_eq!(cfg["mcpServers"][SERVER_NAME]["args"][0], "--mcp");
        assert_eq!(state_in(&cfg, r"c:\apps\DISC-LITE.exe"), State::Connected);
        assert_eq!(state_in(&cfg, r"D:\elsewhere\disc-lite.exe"), State::Outdated);

        let mut cfg = cfg;
        apply(&mut cfg, App::Code, "", false);
        assert!(cfg["mcpServers"].get(SERVER_NAME).is_none());
        assert_eq!(cfg["mcpServers"]["other"]["command"], "x");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_starts_fresh_and_bad_json_is_left_alone() {
        let dir = std::env::temp_dir().join(format!("disc-lite-claude-setup2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_json(&dir.join("nope.json")).unwrap().is_empty());
        std::fs::write(dir.join("bad.json"), "{ not json").unwrap();
        assert!(read_json(&dir.join("bad.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
