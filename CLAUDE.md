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
There is no test suite.

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
- Updates (`update.rs`) read GitHub's latest disc-lite release and pick
  its first `.exe` asset, so every release must attach the exe. Version
  comes from `Cargo.toml` — bump it before building a release. The swap
  renames the running exe to `.old` (Windows allows renaming, not
  overwriting) and the next launch deletes it.
