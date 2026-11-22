//! `disc-lite --mcp`: the Model Context Protocol server Claude starts (over
//! stdin/stdout). It has no library of its own; every call goes to the
//! running Disc Lite through the local bridge in connector.rs, starting Disc
//! Lite first if it isn't open. Song tags are read here, in this process, so
//! the app itself never slows down for them.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::connector::{Info, info_path};

const PROTOCOL: &str = "2025-06-18";

const INSTRUCTIONS: &str = "Disc Lite is the user's music library for video editing. \
Use list_folders to find a folder, list_tracks to see its songs (with title/artist/album tags read from the files), \
and rename_tracks to rename files on disk. Folder arguments accept a folder's key, its name, or its path; \
leave folder out to mean whatever the user is looking at in Disc right now (\"this folder\"). \
When tags are missing, work the title/artist out from the file name, and mention any you had to guess. \
Renames keep the extension, and the user can undo a whole batch with Ctrl+Z in Disc Lite.";

pub fn run() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        // Some clients start the stream with a UTF-8 byte-order mark.
        let line = line.trim_start_matches('\u{feff}');
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            send(&mut out, json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } }));
            continue;
        };
        // Notifications (no id) need no answer.
        let Some(id) = msg.get("id").cloned() else { continue };
        let params = &msg["params"];
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "disc-lite", "title": "Disc Lite", "version": crate::update::VERSION },
                "instructions": INSTRUCTIONS,
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => Ok(call_tool(params["name"].as_str().unwrap_or(""), &params["arguments"])),
            "resources/list" => Ok(json!({ "resources": [] })),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            m => Err(json!({ "code": -32601, "message": format!("method not found: {m}") })),
        };
        let reply = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        };
        send(&mut out, reply);
    }
}

fn send(out: &mut std::io::Stdout, v: Value) {
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn tools() -> Value {
    json!([
        {
            "name": "list_folders",
            "title": "List folders",
            "description": "Lists the folders in Disc Lite's sidebar (in tree order, with keys, names, paths on disk and track counts), and what the user is looking at in Disc right now.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "list_tracks",
            "title": "List tracks",
            "description": "Lists the songs in a Disc folder (including its subfolders), with each file's path, current name, and title/artist/album/track tags read from the file. Leave `folder` out for what's showing in Disc right now.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "folder": { "type": "string", "description": "A folder key, name or path from list_folders; \"all\" for the whole library. Omit for the folder showing in Disc." },
                    "search": { "type": "string", "description": "Only file names containing this text." },
                    "offset": { "type": "integer", "minimum": 0, "description": "Skip this many (for paging)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "At most this many (default 200)." },
                    "tags": { "type": "boolean", "description": "Read title/artist/album tags from the files (default true)." }
                }
            },
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "rename_tracks",
            "title": "Rename tracks",
            "description": "Renames song files on disk (only tracks that are in Disc). `new_name` is the file name without the extension; the extension is kept. Characters Windows doesn't allow in file names (\\ / : * ? \" < > |) are rejected, so replace them first (e.g. \"AC/DC\" -> \"AC-DC\"). Favorites and waveforms carry over, and the user can undo the whole batch with Ctrl+Z in Disc Lite.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "renames": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": { "type": "string", "description": "The track's path, from list_tracks." },
                                "new_name": { "type": "string", "description": "New file name, without extension." }
                            },
                            "required": ["path", "new_name"]
                        }
                    }
                },
                "required": ["renames"]
            },
            "annotations": { "destructiveHint": false, "idempotentHint": true },
        }
    ])
}

fn call_tool(name: &str, args: &Value) -> Value {
    let result = match name {
        "list_folders" => bridge("list_folders", json!({})),
        "list_tracks" => bridge("list_tracks", args.clone()).map(|mut v| {
            if args["tags"].as_bool().unwrap_or(true) {
                add_tags(&mut v);
            }
            v
        }),
        "rename_tracks" => bridge("rename_tracks", args.clone()),
        _ => Err(format!("unknown tool {name}")),
    };
    match result {
        Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }] }),
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

fn add_tags(v: &mut Value) {
    let Some(tracks) = v["tracks"].as_array_mut() else { return };
    for t in tracks {
        let Some(path) = t["path"].as_str().map(str::to_string) else { continue };
        let tags = read_tags(std::path::Path::new(&path));
        t["tags"] = tags;
    }
}

/// Title / artist / album / track number from a file's tags (ID3, Vorbis
/// comments, MP4 atoms, RIFF INFO, ...). Missing ones are left out.
pub fn read_tags(path: &std::path::Path) -> Value {
    use lofty::config::ParseOptions;
    use lofty::file::TaggedFileExt;
    use lofty::tag::Accessor;
    let opts = ParseOptions::new().read_properties(false).read_cover_art(false);
    let Ok(file) = lofty::probe::Probe::open(path).and_then(|p| p.options(opts).read()) else {
        return json!({});
    };
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    let mut put = |k: &str, v: Option<String>| {
        if let Some(v) = v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
            out.insert(k.into(), Value::String(v));
        }
    };
    put("title", tag.title().map(|s| s.into_owned()));
    put("artist", tag.artist().map(|s| s.into_owned()));
    put("album", tag.album().map(|s| s.into_owned()));
    put("genre", tag.genre().map(|s| s.into_owned()));
    if let Some(n) = tag.track() {
        out.insert("track_number".into(), json!(n));
    }
    if let Some(y) = tag.date().map(|d| d.year) {
        out.insert("year".into(), json!(y));
    }
    Value::Object(out)
}

// --- Talking to the app -----------------------------------------------------

enum Failure {
    /// Couldn't reach Disc Lite at all (probably not open).
    Unreachable(String),
    /// Disc Lite answered, with an error.
    App(String),
}

fn bridge(method: &str, params: Value) -> Result<Value, String> {
    match request(method, &params) {
        Ok(v) => Ok(v),
        Err(Failure::App(e)) => Err(e),
        Err(Failure::Unreachable(_)) => {
            open_disc()?;
            wait_for_scan();
            request(method, &params).map_err(|f| match f {
                Failure::App(e) | Failure::Unreachable(e) => e,
            })
        }
    }
}

fn request(method: &str, params: &Value) -> Result<Value, Failure> {
    use Failure::Unreachable as U;
    let text = std::fs::read_to_string(info_path()).map_err(|_| U("Disc Lite isn't running".into()))?;
    let info: Info = serde_json::from_str(&text).map_err(|e| U(e.to_string()))?;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], info.port));
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(|e| U(e.to_string()))?;
    stream.set_read_timeout(Some(Duration::from_secs(60))).ok();
    let mut w = stream.try_clone().map_err(|e| U(e.to_string()))?;
    writeln!(w, "{}", json!({ "token": info.token, "method": method, "params": params })).map_err(|e| U(e.to_string()))?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).map_err(|e| U(e.to_string()))?;
    let reply: Value = serde_json::from_str(&line).map_err(|_| U("Disc Lite closed the connection".into()))?;
    match reply.get("error") {
        Some(e) => Err(Failure::App(e.as_str().unwrap_or("error").to_string())),
        None => Ok(reply["ok"].clone()),
    }
}

/// Starts Disc Lite (this same exe, without --mcp) and waits for it to
/// answer.
fn open_disc() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't open Disc Lite: {e}"))?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(250));
        if request("status", &json!({})).is_ok() {
            return Ok(());
        }
    }
    Err("Opened Disc Lite, but it didn't answer in time. Try again in a moment.".into())
}

/// Right after launch the folders are still being scanned; give that a
/// few seconds so the first answer isn't empty.
fn wait_for_scan() {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        match request("status", &json!({})) {
            Ok(s) if s["scanning"].as_bool() == Some(true) => std::thread::sleep(Duration::from_millis(250)),
            _ => return,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tools_have_schemas() {
        let t = super::tools();
        let names: Vec<&str> = t.as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["list_folders", "list_tracks", "rename_tracks"]);
        for x in t.as_array().unwrap() {
            assert_eq!(x["inputSchema"]["type"], "object");
        }
    }
}
