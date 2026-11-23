# CLAUDE.md

Disc Lite: a deliberately minimal fork of Disc, rewritten in Rust with a
native egui UI (eframe 0.37, glow renderer). Read `README.md` for what's in
it and the file layout. Keep it simple — resist adding panels, themes,
settings, or other features; that's what regular Disc is for.

## Commands
- `cargo build` — the check to run after changes
- `cargo build --release` — the shipped exe (`target/release/disc-lite.exe`;
  fat LTO, takes a few minutes)
- `DISC_LITE_DATA=<dir>` runs against a throwaway settings folder
`cargo test` runs the unit tests (folders, connector, Claude setup, updater, waveform).

## Things that aren't obvious
- All state lives in `DiscApp` (`src/app.rs`); the `ui_*` modules collect
  clicks into an `Action` list and apply them after drawing, which keeps the
  borrow checker out of the way.
- Scanning, watching, and waveform decoding all run on background threads and
  report back over channels; the UI thread never touches the disk in a frame
  except for small settings writes (debounced).
- The app only repaints on input or while something moves; during playback
  it's ~30fps focused, 4fps unfocused. Keep it that way.
- Drag-out (`dragout.rs`) runs Windows' blocking OLE drag loop inside a
  frame. egui never sees the mouse release, so `raw_input_hook` injects one.
- Row hover uses `ui.rect_contains_pointer(rect)`, not `resp.hovered()`:
  hovered() goes false over a child button (×, heart), which made those
  buttons flicker out from under the click.
- Removing or moving folders must only change Disc's own list — never files
  on disk.
- The eframe `wgpu` default renderer doubled memory use; it's `glow` on
  purpose.
- Updates (`update.rs`) are checked once at launch (silently; a failed
  check shows nothing). They read GitHub's latest disc-lite release and pick
  its first `.exe` asset, so every release must attach the exe. Version
  comes from `Cargo.toml` — bump it before building a release. The swap
  renames the running exe to `.old` (Windows allows renaming, not
  overwriting) and the next launch deletes it.
- Colors are functions (`theme::TEXT()`), not constants, so a theme switch
  applies next frame. A new theme is one more `Palette` in `theme::THEMES`;
  a new color is a field on `Palette` plus its accessor fn.
- The window is undecorated; `titlebar.rs` draws the title bar. Moving
  goes through `ViewportCommand::StartDrag` (Windows' own move loop, so
  edge-snapping works), edges resize via `BeginResize`, and maximized
  windows need no extra inset (checked against a real screen capture —
  `GetWindowRect` includes the invisible shadow, which is misleading).
  `visuals.panel_fill` is the lighter panel color on purpose: it shows
  through the track list's rounded corner.
- winit leaves a 1px non-client strip at the top of frameless-with-shadow
  windows (its shadow hack). `titlebar::no_top_gap` swaps in a window proc
  that answers WM_NCCALCSIZE with "client = whole window" (except when
  maximized) and gets the shadow via a 1px *bottom* DWM frame extension
  instead (a top one would make DWM draw a caption).
- The Claude connector has three parts. `connector.rs` is in the app: a
  127.0.0.1 listener (random port + token, advertised in `connector.json` in
  the data folder) answering from a `Snapshot` the app publishes on every
  view rebuild, so Claude still gets answers while Disc is minimized (eframe
  may not run frames then). Renames happen on the bridge thread, which then
  sends `Event::Renamed` so the app carries favorites, removed tracks, the
  queue and waveforms over to the new paths, and records an undo step.
  `mcp.rs` is `disc-lite --mcp`, the stdio MCP server Claude launches. It
  proxies to the bridge, starts Disc if it isn't open, and reads tags
  (lofty) in its own process. `claude_setup.rs` writes only our
  `mcpServers.disc-lite` entry into the Claude app's config (the
  Microsoft Store build reads it from `Packages\Claude_*\LocalCache\Roaming\Claude`)
  and Claude Code's `~/.claude.json`, keeping key order (serde_json
  `preserve_order`) and making a one-time `.before-disc-lite` backup.
  Claude can only rename tracks that are already in Disc.
- Ctrl+Z is one stack (`UndoStep`) covering remove, restore and rename
  (rename undo includes Claude's whole batch).
- A directory can be both an added folder and a scanned subfolder of
  another added folder (Add Folder Inside… on a real subdirectory). The
  deepest added folder owns those tracks; `tree::build_model` drops them
  from the outer folder (`nested_added_dirs`) so the tree doesn't show the
  directory twice.
