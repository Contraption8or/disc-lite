import { app, BrowserWindow, ipcMain, dialog, nativeImage, nativeTheme, Menu, shell, clipboard, protocol } from "electron";
import path from "node:path";
import fs from "node:fs/promises";
import { watch, appendFileSync, createReadStream } from "node:fs";
import { fileURLToPath } from "node:url";
import { Readable } from "node:stream";

// Dev-only escape hatch so a second checkout (a git worktree used for
// testing a branch, for instance) doesn't share userData — and therefore
// localStorage and device-identity.json — with whatever install is
// already using the default profile. Without this, an untested branch
// could silently read/write real data, including emitting Studio Sync
// events under the real device's identity into a real shared folder.
if (process.env.DISC_USER_DATA) {
  app.setPath("userData", process.env.DISC_USER_DATA);
}

// Where Disc's own releases are published — used by the update check
// below. Public repo, so the GitHub API needs no auth for this.

// Content-Type by extension for disc-media:// responses (see below) — the
// same set of formats the rest of the app already treats as playable
// (Chromium's Web Audio decodes all of these natively). Falls back to
// audio/mpeg for anything unrecognized, same as the old Blob-based
// playback path did.
const MEDIA_MIME_TYPES = {
  ".mp3": "audio/mpeg",
  ".wav": "audio/wav",
  ".m4a": "audio/mp4",
  ".aac": "audio/aac",
  ".ogg": "audio/ogg",
  ".opus": "audio/opus",
  ".flac": "audio/flac",
  ".webm": "audio/webm",
  // The Image panel (see ImagePanel.jsx) streams its picture through this
  // same protocol rather than storing image bytes in localStorage.
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".avif": "image/avif",
  ".bmp": "image/bmp",
  ".svg": "image/svg+xml",
};

function mediaMimeType(filePath) {
  return MEDIA_MIME_TYPES[path.extname(filePath).toLowerCase()] || "audio/mpeg";
}

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDev = process.env.NODE_ENV === "development";

// Playback used to read a track's entire file into memory, ship every byte
// across IPC to the renderer, and build a Blob from it before <audio> could
// even start — for a multi-MB file that's real, measurable latency on every
// track switch, and it's redone from scratch each time. This custom scheme
// lets <audio src="disc-media://..."> stream straight from disk instead
// (net.fetch on a file:// URL gets Range-request/streaming support for
// free), so playback can start as soon as the first chunk is available
// rather than waiting on the whole file. Must be registered before the app
// is ready. "standard: true" + "stream: true" are what let range requests
// and progressive playback work; "supportFetchAPI" isn't needed since
// nothing calls fetch() against it directly.
protocol.registerSchemesAsPrivileged([
  {
    scheme: "disc-media",
    privileges: { standard: true, secure: true, stream: true, bypassCSP: true, corsEnabled: true },
  },
]);

// Without this, launching Disc while it's already running doesn't do
// nothing so much as it does something confusing: a second full process
// tree (Vite + Electron) spins up and immediately collides with the first
// over port 5174, and depending on timing that can either silently fail
// or leave stray processes behind — repeated launches this way is
// genuinely how a machine ends up with a pile of stuck node/electron
// processes and a Desktop shortcut that looks like it's doing nothing.
// requestSingleInstanceLock() is the standard Electron fix: a second
// launch attempt hands off to the first instance (which just focuses its
// window, see the "second-instance" listener below) and quits
// immediately instead of trying to run alongside it.
const gotSingleInstanceLock = app.requestSingleInstanceLock();
if (!gotSingleInstanceLock) {
  app.quit();
}

// Windows identifies running apps for taskbar pinning/grouping/
// notifications by this id, not by window title or icon — without it,
// every Electron app running from the same generic node_modules/electron/
// dist/electron.exe (true for any dev-mode Electron app, not just this
// one) is indistinguishable to Windows, which is very likely why pinning
// straight from the running taskbar button falls back to Electron's own
// generic icon regardless of what icon the BrowserWindow itself was given.
// Deliberately its own id, distinct from package.json's build.appId
// ("com.disc.lite") — that id is what a real packaged install registers
// with Windows (Start Menu entry, Programs-and-Features entry, icon), and
// a dev-mode instance squatting on the same id can end up with a blank/
// generic taskbar icon if that packaged install is ever removed while
// Windows still has this id cached against it. A distinct id sidesteps
// that regardless of what is or isn't installed.
if (process.platform === "win32") {
  app.setAppUserModelId("com.disc.lite");
}

let mainWindow = null;
const folderWatchers = new Map(); // key -> FSWatcher
const watchDebounceTimers = new Map(); // key -> Timeout

// Where render-process crashes get logged.
const crashLogPath = path.join(app.getPath("userData"), "crash-log.jsonl");

// Where a marked section's trimmed clip lives for dragging out to Premiere
// (see src/audio/sectionDrag.js) — a ".disc-sections" subfolder right next
// to the source track itself, not a temp directory. It used to live under
// the OS temp dir and get wiped on every launch, which meant a clip
// already sitting in a Premiere project would go offline as soon as Disc
// (or the OS) cleared temp — dragging into a timeline expects that file to
// keep existing. Named with a leading dot, and explicitly skipped by the
// folder scanner below, so it never shows up as a real library track.
const SECTIONS_DIR_NAME = ".disc-sections";
// Folder used inside a shared/synced music directory for the event log —
// same "dot-prefixed, explicitly skipped by the scanner" pattern as
// SECTIONS_DIR_NAME above, so it never shows up as library content.
const SYNC_DIR_NAME = ".disc-sync";

// Used as the little icon that follows the cursor during a native
// drag-out (e.g. dragging a track's waveform into Premiere Pro).
const dragIcon = nativeImage.createFromPath(
  path.join(__dirname, "assets", "drag-icon.png")
);

// A plain window with no File/Edit/View menu bar — just the native title bar.
Menu.setApplicationMenu(null);
// Follow a dark title bar (this app is black).
nativeTheme.themeSource = "dark";

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 760,
    minHeight: 480,
    backgroundColor: "#000000",
    title: "Disc Lite",
    // Without this, dev mode (`electron .`, not a packaged build) shows
    // Electron's own generic default icon in the taskbar instead of
    // Disc's — a packaged build gets this for free from the .exe's own
    // embedded icon resource (see package.json's build.win config), but
    // that only applies once actually built; running from source needs
    // it set explicitly.
    icon: path.join(__dirname, "assets", "disc-icon.ico"),
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      // Needed later for native drag-out of files into Premiere Pro.
      // Left here as a marker for Phase 3 (drag-to-Premiere).
    },
  });

  // Belt-and-suspenders alongside the `icon:` constructor option above —
  // harmless, and keeps the window's icon explicitly set on the live
  // native handle rather than relying solely on constructor-time state.
  mainWindow.setIcon(nativeImage.createFromPath(path.join(__dirname, "assets", "disc-icon.ico")));

  if (isDev) {
    mainWindow.loadURL("http://localhost:5174");
    mainWindow.webContents.openDevTools({ mode: "detach" });
  } else {
    mainWindow.loadFile(path.join(__dirname, "..", "dist", "index.html"));
  }

  // Keep the window title as "Disc Lite" — Electron would otherwise swap in
  // the page's own <title> on every navigation.
  mainWindow.on("page-title-updated", (event) => {
    event.preventDefault();
  });

  // If the renderer crashes, log it, then offer Reload/Close in a native
  // dialog (native because it doesn't depend on the crashed page).
  mainWindow.webContents.on("render-process-gone", (_event, details) => {
    try {
      const entry = {
        ts: new Date().toISOString(),
        reason: details.reason,
        exitCode: details.exitCode,
      };
      appendFileSync(crashLogPath, JSON.stringify(entry) + "\n", "utf8");
    } catch {
      // Logging the crash is best-effort — nothing to do if even that fails.
    }
    if (!mainWindow) return;
    dialog
      .showMessageBox(mainWindow, {
        type: "error",
        title: "Disc crashed",
        message: `Disc's window stopped responding (${details.reason}).`,
        detail:
          "This has been logged. Reload to keep working, or close the window.",
        buttons: ["Reload", "Close"],
        defaultId: 0,
        cancelId: 1,
      })
      .then((result) => {
        if (!mainWindow) return;
        if (result.response === 0) {
          if (isDev) mainWindow.loadURL("http://localhost:5174");
          else mainWindow.loadFile(path.join(__dirname, "..", "dist", "index.html"));
        } else {
          mainWindow.close();
        }
      });
  });

  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

// The losing instance already called app.quit() above — this just makes
// sure it never gets as far as opening a second window in the meantime.
app.on("second-instance", () => {
  if (!mainWindow) return;
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.show();
  mainWindow.focus();
});

app.whenReady().then(() => {
  if (!gotSingleInstanceLock) return;

  // The URL is disc-media://play/<encoded absolute file path>. "play" as
  // the host is arbitrary (custom schemes need one) — everything that
  // matters is in the path, which is exactly the file path run through
  // encodeURIComponent so path separators, drive-letter colons, spaces,
  // and unicode filenames all survive intact.
  //
  // This used to just be `net.fetch(pathToFileURL(filePath).href, ...)` —
  // simpler, but it left Chromium unable to tell the resource was actually
  // seekable (no reliable Accept-Ranges/Content-Range on the response), so
  // every attempt to seek got silently reset back to 0: <audio>.currentTime
  // would read back 0 immediately after being set, seeking/seeked fired but
  // landed at ~0 either way. Building the response by hand — reading only
  // the requested byte range via a real fs stream, and setting
  // Content-Range/Accept-Ranges/206 ourselves — is what actually makes
  // seeking work, rather than hoping net.fetch infers the right semantics
  // for a local file.
  protocol.handle("disc-media", async (request) => {
    try {
      const encoded = new URL(request.url).pathname.replace(/^\/+/, "");
      const filePath = decodeURIComponent(encoded);
      const stat = await fs.stat(filePath);
      const fileSize = stat.size;
      const contentType = mediaMimeType(filePath);

      const rangeHeader = request.headers.get("range");
      if (rangeHeader) {
        const match = /bytes=(\d*)-(\d*)/.exec(rangeHeader);
        let start = match?.[1] ? parseInt(match[1], 10) : 0;
        let end = match?.[2] ? parseInt(match[2], 10) : fileSize - 1;
        if (!Number.isFinite(start) || start < 0) start = 0;
        if (!Number.isFinite(end) || end >= fileSize) end = fileSize - 1;
        // A genuinely invalid range (start past the end of the file, or
        // past what it resolves to after clamping — including the
        // zero-byte-file case, where end lands at -1) has no sane byte
        // range to fall back to. The previous version reset `start` to 0
        // without touching `end`, which could still leave start > end and
        // hand createReadStream a nonsensical range instead of properly
        // rejecting it.
        if (start >= fileSize || start > end) {
          return new Response(null, {
            status: 416,
            headers: { "Content-Range": `bytes */${fileSize}` },
          });
        }

        const stream = createReadStream(filePath, { start, end });
        return new Response(Readable.toWeb(stream), {
          status: 206,
          headers: {
            "Content-Type": contentType,
            "Content-Range": `bytes ${start}-${end}/${fileSize}`,
            "Content-Length": String(end - start + 1),
            "Accept-Ranges": "bytes",
          },
        });
      }

      const stream = createReadStream(filePath);
      return new Response(Readable.toWeb(stream), {
        status: 200,
        headers: {
          "Content-Type": contentType,
          "Content-Length": String(fileSize),
          "Accept-Ranges": "bytes",
        },
      });
    } catch {
      return new Response("Not found", { status: 404 });
    }
  });

  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
}).catch((err) => {
  console.error("Failed to start Disc:", err);
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

// --- IPC handlers -----------------------------------------------------

// --- Profiles ---------------------------------------------------------
// A "profile" is everything Disc persists (theme, folders, tags, notes,
// shortcuts, appearance, layout, marked sections, all of it) bundled
// into one JSON file. Since Disc doesn't hold the actual music files
// itself, a profile is what makes a whole setup shareable — send someone
// the file and they get your folder structure, tags, and settings
// without needing any of the same files to already be organized the
// same way (though the linked folder paths obviously still need to
// exist on their machine to actually show tracks).
//
// Each profile is its own file, named by a stable generated id rather
// than its display name — renaming a profile only ever means rewriting
// the "profileName" field inside the file, never touching the filename
// itself, which sidesteps every filesystem-rename edge case (illegal
// characters, collisions, case-sensitivity differences across
// platforms) that a name-as-filename scheme would run into.

// Toggle "always on top" — the pin button in the bottom bar.
ipcMain.handle("disc:toggle-always-on-top", (_event, shouldPin) => {
  if (!mainWindow) return false;
  mainWindow.setAlwaysOnTop(shouldPin, "floating");
  return mainWindow.isAlwaysOnTop();
});

// A native multi-select file picker for adding mp3s — a reliable
// alternative to drag-and-drop, since OS-level DND can get intercepted by
// third-party window-manager tools (WindHawk mods and similar) in ways
// Disc has no visibility into or control over.
// Native multi-select folder picker — the sidebar's "+" uses this so a whole
// set of folders can be added in one go.
ipcMain.handle("disc:choose-folders", async () => {
  if (!mainWindow) return [];
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ["openDirectory", "multiSelections"],
    title: "Add music folders",
  });
  if (result.canceled) return [];
  return result.filePaths;
});

ipcMain.handle("disc:choose-mp3-files", async () => {
  if (!mainWindow) return [];
  const result = await dialog.showOpenDialog(mainWindow, {
    properties: ["openFile", "multiSelections"],
    title: "Choose mp3s, wavs, oggs, or mov clips to add",
    filters: [{ name: "Audio & Video", extensions: ["mp3", "wav", "ogg", "mov"] }],
  });
  if (result.canceled) return [];
  return result.filePaths;
});

function getFileType(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(".mov")) return "video";
  if (lower.endsWith(".mp3") || lower.endsWith(".wav") || lower.endsWith(".ogg")) return "audio";
  return null;
}

// Recursively walk a folder and return every .mp3/.mov file it finds,
// along with its path relative to the root (used to figure out which
// subfolder a track lives in). Returns null (not []) if the root itself
// doesn't exist/isn't reachable — e.g. an external drive that's been
// unplugged — so the renderer can tell "genuinely empty" apart from
// "temporarily unreachable" and avoid dropping tracks from view.
async function scanForMp3s(rootDir) {
  try {
    await fs.access(rootDir);
  } catch {
    return null;
  }

  const results = [];

  async function walk(currentDir) {
    let entries;
    try {
      entries = await fs.readdir(currentDir, { withFileTypes: true });
    } catch {
      return;
    }

    for (const entry of entries) {
      const fullPath = path.join(currentDir, entry.name);
      const fileType = entry.isFile() ? getFileType(entry.name) : null;
      if (entry.isDirectory()) {
        // Marked-section clips (a feature this build no longer has, but an older
        // Disc may have left behind) live right next
        // to their source track, inside a folder a normal scan would
        // otherwise happily walk into and list as real library tracks.
        if (entry.name === SECTIONS_DIR_NAME || entry.name === SYNC_DIR_NAME) continue;
        await walk(fullPath);
      } else if (fileType) {
        let sizeBytes = 0;
        let addedAtMs = null;
        try {
          const stat = await fs.stat(fullPath);
          sizeBytes = stat.size;
          // birthtime isn't reliable on every filesystem (some report the
          // same value as mtime, or epoch 0) — fall back to mtime then.
          addedAtMs =
            stat.birthtimeMs && stat.birthtimeMs > 0 ? stat.birthtimeMs : stat.mtimeMs;
        } catch {
          // File may have been removed mid-scan; skip its stats silently.
        }
        const relativeDir = path.relative(rootDir, currentDir);
        results.push({
          id: fullPath,
          filePath: fullPath,
          fileName: entry.name,
          fileType,
          relativeDir: relativeDir === "" ? null : relativeDir,
          sizeBytes,
          addedAtMs,
        });
      }
    }
  }

  await walk(rootDir);
  return results;
}

ipcMain.handle("disc:scan-folder", async (_event, folderPath) => {
  if (!folderPath) return [];
  return scanForMp3s(folderPath);
});

// Reads a file's raw bytes so the renderer can play it (via a Blob URL)
// and decode its waveform (via Web Audio) without needing file:// access,
// which is unreliable from a Vite dev server origin.
ipcMain.handle("disc:read-audio-file", async (_event, filePath) => {
  try {
    const buffer = await fs.readFile(filePath);
    // Return a plain Uint8Array (not a Node Buffer) so it structured-clones
    // cleanly across the context bridge and works directly with Blob().
    return new Uint8Array(buffer);
  } catch {
    return null;
  }
});

// --- Audio → MP3 converter -------------------------------------------
// Decoding and encoding both happen in the renderer (Chromium decodes
// all of these natively via Web Audio — the same pipeline already used
// everywhere else in Disc for waveforms; lamejs — a pure-JS encoder, no
// native binary — handles the MP3 side). The main process's job here is
// just picking files/folders and writing the finished bytes to disk.
// This list is deliberately limited to formats Chromium's Web Audio
// reliably decodes — leaving out things like WMA that aren't a
// web-standard format and can't be promised to work.

// Renames a track's actual file on disk, keeping its original extension
// (the rename UI only ever lets you edit the name without the extension,
// same as how it's displayed everywhere else — this just enforces that
// on the backend too, so there's no way to accidentally give a file the
// wrong extension). Strips characters that are illegal in filenames on
// Windows even if they'd be fine on the OS Disc happens to be running on,
// since library folders often get shared/moved across machines.
ipcMain.handle("disc:rename-track-file", async (_event, { filePath, newStem }) => {
  try {
    const dir = path.dirname(filePath);
    const ext = path.extname(filePath);
    const safeName = newStem.replace(/[/\\:*?"<>|]/g, "").trim();
    if (!safeName) return { success: false, error: "Name can't be empty" };

    const newPath = path.join(dir, `${safeName}${ext}`);
    if (newPath === filePath) return { success: true, newPath };

    try {
      await fs.access(newPath);
      return { success: false, error: "A file with that name already exists" };
    } catch {
      // Doesn't exist yet — good, proceed.
    }

    await fs.rename(filePath, newPath);
    return { success: true, newPath };
  } catch (err) {
    return { success: false, error: err.message };
  }
});

// Case-insensitive on Windows (the only platform this app ships for), a
// straight string compare everywhere else — used below to catch "this
// drop target *is* where the file already lives" before it ever reaches
// the rename-on-collision loop.
function samePath(a, b) {
  const na = path.resolve(a);
  const nb = path.resolve(b);
  return process.platform === "win32" ? na.toLowerCase() === nb.toLowerCase() : na === nb;
}

// Copy mp3s dragged into the Disc window from the OS into the chosen
// music folder. The folder watcher then picks the new files up on its own.
ipcMain.handle("disc:copy-files-into-folder", async (_event, { folderPath, sourcePaths }) => {
  const copied = [];
  const skipped = [];
  if (!folderPath || !Array.isArray(sourcePaths)) return { copied, skipped };

  for (const sourcePath of sourcePaths) {
    if (!getFileType(sourcePath)) {
      skipped.push(sourcePath);
      continue;
    }

    const baseName = path.basename(sourcePath);

    // Dragging a track back into the very folder it's already sitting in
    // isn't an import at all — without this check, the collision loop
    // below sees a file already at the destination (itself) and
    // "resolves" that the same way it would a genuine naming collision:
    // by copying the source onto a new "<name> (1).ext", producing a
    // pointless duplicate of the exact same file.
    if (samePath(sourcePath, path.join(folderPath, baseName))) {
      skipped.push(sourcePath);
      continue;
    }

    const ext = path.extname(baseName);
    const stem = path.basename(baseName, ext);
    let destName = baseName;
    let destPath = path.join(folderPath, destName);
    let counter = 1;

    // Don't clobber a file that's already there — rename with " (1)", " (2)"...
    while (true) {
      try {
        await fs.access(destPath);
        destName = `${stem} (${counter})${ext}`;
        destPath = path.join(folderPath, destName);
        counter += 1;
      } catch {
        break;
      }
    }

    // Copying (not moving) so the original file, wherever it was dragged
    // from, is left untouched.
    try {
      await fs.copyFile(sourcePath, destPath);
      copied.push(destPath);
    } catch {
      skipped.push(sourcePath);
    }
  }

  return { copied, skipped };
});

// Watch a folder so new/removed mp3s are picked up automatically instead
// of requiring a manual rescan. Each root Disc cares about (the main music
// folder, plus any linked custom-folder directories) gets its own watcher,
// identified by `key` — "main" for the music folder, or a custom folder's
// id. We just notify the renderer that something changed under that key
// and let it re-run disc:scan-folder for that specific root.
ipcMain.handle("disc:watch-folder", (_event, { key, folderPath }) => {
  const existing = folderWatchers.get(key);
  if (existing) {
    existing.close();
    folderWatchers.delete(key);
  }
  if (!folderPath || !mainWindow) return false;

  try {
    const watcher = watch(
      folderPath,
      { recursive: true },
      () => {
        clearTimeout(watchDebounceTimers.get(key));
        watchDebounceTimers.set(
          key,
          setTimeout(() => {
            mainWindow?.webContents.send("disc:folder-changed", key);
          }, 400)
        );
      }
    );
    // fs.watch's returned watcher is an EventEmitter — an unhandled
    // 'error' event on any EventEmitter throws in Node by default, and
    // that would crash this whole process, not just fail this one watch.
    // A watched directory disappearing mid-session (an external/network
    // drive getting disconnected — exactly what Disc's "missing folder"
    // banner already exists to handle) is a real, expected way for this
    // to fire, so it needs a real handler, not silence.
    watcher.on("error", () => {
      folderWatchers.delete(key);
      mainWindow?.webContents.send("disc:folder-changed", key);
    });
    folderWatchers.set(key, watcher);
    return true;
  } catch {
    // Recursive watching isn't supported on every platform/filesystem;
    // the person can still hit a manual rescan if this silently fails.
    return false;
  }
});

ipcMain.handle("disc:unwatch-folder", (_event, key) => {
  const existing = folderWatchers.get(key);
  if (existing) {
    existing.close();
    folderWatchers.delete(key);
  }
  clearTimeout(watchDebounceTimers.get(key));
  watchDebounceTimers.delete(key);
  return true;
});

// Native OS drag-out — this is what lets a track's waveform be dragged
// straight onto Premiere Pro's timeline (or anywhere else that accepts a
// dropped file), the same as dragging it out of Explorer. Must be
// triggered via ipcRenderer.send (not invoke) and handled synchronously:
// startDrag needs to run in the same tick as the renderer's dragstart
// event, so the async round-trip of invoke/handle would break it.
ipcMain.on("disc:start-drag", (event, filePath) => {
  if (!filePath) return;
  event.sender.startDrag({
    file: filePath,
    icon: dragIcon,
  });
});

// Shows the file selected in Explorer — the "reveal in Explorer" action.
ipcMain.on("disc:reveal-in-explorer", (_event, filePath) => {
  if (filePath) shell.showItemInFolder(filePath);
});

ipcMain.on("disc:copy-to-clipboard", (_event, text) => {
  if (typeof text === "string") clipboard.writeText(text);
});

// Deletes a track from disk — moves it to the OS trash/recycle bin rather
// than permanently unlinking it, so it's recoverable if this was a mistake
// (e.g. clicked the wrong duplicate).
ipcMain.handle("disc:delete-file", async (_event, filePath) => {
  if (!filePath) return false;
  try {
    await shell.trashItem(filePath);
    return true;
  } catch {
    return false;
  }
});

// Physically moves a file on disk into a different folder — used by the
// multi-select "Move to folder" batch action. Same collision-safe
// renaming as the drag-and-drop copy handler, but moves rather than
// copies since this is reorganizing files that are already in the library.
ipcMain.handle("disc:move-file", async (_event, { sourcePath, destFolderPath }) => {
  if (!sourcePath || !destFolderPath) return null;

  const baseName = path.basename(sourcePath);
  const ext = path.extname(baseName);
  const stem = path.basename(baseName, ext);
  let destName = baseName;
  let destPath = path.join(destFolderPath, destName);
  let counter = 1;

  while (true) {
    try {
      await fs.access(destPath);
      if (path.resolve(destPath) === path.resolve(sourcePath)) break; // moving onto itself
      destName = `${stem} (${counter})${ext}`;
      destPath = path.join(destFolderPath, destName);
      counter += 1;
    } catch {
      break;
    }
  }

  try {
    await fs.rename(sourcePath, destPath);
  } catch {
    // rename() fails across different drives — fall back to copy+delete.
    try {
      await fs.copyFile(sourcePath, destPath);
      await fs.unlink(sourcePath);
    } catch {
      return null;
    }
  }
  return destPath;
});
