//! "Check for updates": asks GitHub for the newest disc-lite release, and
//! if it's newer, downloads its .exe and swaps it in for the running one.
//!
//! Windows lets a running exe be renamed (just not overwritten), so the
//! swap is: download to `<exe>.new`, rename the running exe to `<exe>.old`,
//! rename `.new` into its place. The new version starts once this one has
//! saved and exited (see `DiscApp::on_exit`), and it deletes `.old` on its
//! first launch. If the exe's folder isn't writable, the release page is
//! opened in the browser instead.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui;

const REPO: &str = "Contraption8or/disc-lite";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub asset_url: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(String),
    /// The new exe is in place; restart into it.
    Ready(PathBuf),
    Failed(String),
}

pub struct Updater {
    status: Arc<Mutex<Status>>,
}

fn user_agent() -> String {
    format!("disc-lite/{VERSION}")
}

/// "v2.1.0" / "2.1.0" -> [2, 1, 0]
fn parse_version(s: &str) -> Vec<u64> {
    s.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.trim().parse().unwrap_or(0)).collect()
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, mut b) = (parse_version(latest), parse_version(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

fn old_path(exe: &std::path::Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

fn new_path(exe: &std::path::Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".new");
    PathBuf::from(s)
}

impl Updater {
    pub fn new() -> Self {
        // Leftover from the update that launched this version.
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::fs::remove_file(old_path(&exe));
            let _ = std::fs::remove_file(new_path(&exe));
        }
        Self { status: Arc::new(Mutex::new(Status::Idle)) }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn reset(&self) {
        *self.status.lock().unwrap() = Status::Idle;
    }

    pub fn check(&self, ctx: &egui::Context) {
        let status = self.status.clone();
        if matches!(*status.lock().unwrap(), Status::Checking | Status::Downloading(_)) {
            return;
        }
        *status.lock().unwrap() = Status::Checking;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = fetch_latest().map(|rel| {
                if is_newer(&rel.version, VERSION) { Status::Available(rel) } else { Status::UpToDate }
            });
            // This runs by itself at launch, so a failed check (offline, GitHub
            // down) stays quiet: no update button, no message.
            *status.lock().unwrap() = result.unwrap_or(Status::Idle);
            ctx.request_repaint();
        });
    }

    pub fn install(&self, ctx: &egui::Context, rel: Release) {
        let Some(url) = rel.asset_url.clone() else {
            open_url(&rel.page);
            return;
        };
        let status = self.status.clone();
        *status.lock().unwrap() = Status::Downloading(rel.version.clone());
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = download_and_swap(&url);
            *status.lock().unwrap() = match result {
                Ok(exe) => Status::Ready(exe),
                Err(e) => {
                    // Can't replace it ourselves (read-only folder, etc):
                    // let the browser download it.
                    open_url(&rel.page);
                    Status::Failed(format!("{e} — opened the download page instead"))
                }
            };
            ctx.request_repaint();
        });
    }
}

fn fetch_latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let text = ureq::get(&url)
        .header("User-Agent", &user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub ({e})"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let version = json["tag_name"].as_str().ok_or("No release found")?.trim_start_matches(['v', 'V']).to_string();
    let page = json["html_url"].as_str().unwrap_or("https://github.com/Contraption8or/disc-lite/releases").to_string();
    let asset_url = json["assets"].as_array().and_then(|assets| {
        assets
            .iter()
            .find(|a| a["name"].as_str().is_some_and(|n| n.to_ascii_lowercase().ends_with(".exe")))
            .and_then(|a| a["browser_download_url"].as_str())
            .map(String::from)
    });
    Ok(Release { version, page, asset_url })
}

fn download_and_swap(url: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bytes = ureq::get(url)
        .header("User-Agent", &user_agent())
        .call()
        .map_err(|e| format!("Download failed ({e})"))?
        .body_mut()
        .with_config()
        .limit(200 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("Download failed ({e})"))?;
    // Sanity check: a Windows executable, and not a truncated one.
    if bytes.len() < 1024 * 1024 || !bytes.starts_with(b"MZ") {
        return Err("The download wasn't a valid app file".into());
    }
    let new = new_path(&exe);
    let old = old_path(&exe);
    let mut f = std::fs::File::create(&new).map_err(|e| format!("Couldn't save the update ({e})"))?;
    f.write_all(&bytes).map_err(|e| format!("Couldn't save the update ({e})"))?;
    drop(f);
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(&exe, &old) {
        let _ = std::fs::remove_file(&new);
        return Err(format!("Couldn't replace the app ({e})"));
    }
    if let Err(e) = std::fs::rename(&new, &exe) {
        // Put the original back so nothing is left broken.
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("Couldn't replace the app ({e})"));
    }
    Ok(exe)
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_versions() {
        assert!(is_newer("2.2.0", "2.1.0"));
        assert!(is_newer("v2.10.0", "2.9.9"));
        assert!(is_newer("3.0", "2.9.9"));
        assert!(!is_newer("2.1.0", "2.1.0"));
        assert!(!is_newer("v2.0.5", "2.1.0"));
    }
}
