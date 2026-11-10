# Disc Lite

A stripped-down, black-and-white version of [Disc](https://github.com/Contraption8or/Disc): just your music
folders, waveform rows, and drag-to-Premiere. No themes, panels, tags,
collections, timers, or settings screens.

Version 2 is a ground-up rewrite in Rust with a fully native UI (egui), so
there's no browser engine inside: one ~7.5 MB `.exe`, a window in about a
quarter of a second, and a fraction of the Electron version's memory.

## What's in it

- **One fixed-width folder column** on the left (260px, never resizes with the
  window): *All Music*, *Favorites*, then a VS Code-style folder tree. The
  folder icon with `+` adds folders (multi-select); double-click a folder to
  rename it; hover for `×` to remove it (files on disk are never touched).
- **Move folders around** by dragging: drop on the top/bottom edge of a row to
  put it before/after, on the middle to nest it inside. Hovering a collapsed
  folder opens it; `Esc` cancels. Shift/Ctrl-click to select several, then
  drag them together or remove them all from Disc. Moves only change Disc's
  list — nothing on disk moves.
- **Track list** with a waveform per row. Click the waveform to seek, **drag
  it out onto Premiere's timeline** (or Explorer, a DAW, ...), heart to
  favorite, right-click for Rename / Copy File Path / Reveal in Explorer /
  Remove from Disc (hides it; the file stays put, `Ctrl+Z` brings it back) /
  Delete (to the Recycle Bin).
- **Play** whatever's shown, with a bottom bar (prev, play, next, loop,
  shuffle, volume, pin on top). Play follows the Shuffle toggle; with it on,
  the queue reshuffles and keeps going at the end. Loop repeats the current
  track.
- **Search** across all music. Drop audio files onto the window (or use
  *Import*) to copy them into the folder you're viewing.
- Keys: `Space` play/pause · `←`/`→` seek 5s · `↑`/`↓` volume · `S` shuffle · `L` loop · `Ctrl+Z` undo remove ·
  `Ctrl+F` or `/` search · in the folder column: `Delete` remove selected,
  `Ctrl+A` select all, `Esc` clear selection.

**Themes**: the small palette icon next to the version number (bottom of
the folder column) switches between *Mono* (black and white) and *Plum*
(dusky wine with a peach accent). The choice is remembered.

**Updates**: Disc Lite quietly checks GitHub for a newer release when it
launches. If there is one, an *Update to X* button appears at the bottom of
the folder column: one click downloads it,
swaps it in for the running exe, and restarts Disc Lite (if the exe's folder
isn't writable, it opens the download page instead).

On first launch it imports folders, favorites, the folder arrangement,
volume, and shuffle from the old Electron Disc Lite (read-only — the old app's
data is left as it was).

## Build

Needs a Rust toolchain (`rustup`, stable).

```
cargo run              # debug build
cargo build --release  # target/release/disc-lite.exe — the whole app
```

Settings live in `%APPDATA%\Disc Lite\disc-lite-settings.json`, waveform
peaks in `waveforms.cache` next to it. Set `DISC_LITE_DATA` to a folder to
run against a throwaway profile instead.

## Layout

- `src/main.rs` — window setup
- `src/app.rs` — all app state and behavior (library, playback queue,
  folders, files, shortcuts, saving)
- `src/ui_sidebar.rs`, `src/ui_tracks.rs`, `src/ui_player.rs` — the three
  areas of the window
- `src/tree.rs` — the folder tree model and moves (same `placement`/`order`
  format the Electron version used)
- `src/library.rs` — background scanning, file watching, file operations
- `src/waveform.rs` — background waveform decoding + disk cache
- `src/audio.rs` — playback (rodio); `src/dragout.rs` — native drag-out
- `src/migrate.rs` — one-time import from the Electron version
- `src/update.rs` — update check at launch and the in-place exe swap
- `src/theme.rs`, `src/icons.rs`, `src/widgets.rs` — themes, painted
  icons, shared widgets
