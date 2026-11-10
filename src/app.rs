use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::audio::Audio;
use crate::library::{self, Library, ScanResult};
use crate::model::{Folder, TrackRef, folder_name_from_path, new_folder_id};
use crate::persist::{self, Settings};
use crate::tree::{self, Model, parse_selection};
use crate::waveform::Waveforms;
use crate::widgets::{Confirm, ConfirmResult};
use crate::{migrate, theme};

/// What a pending confirmation will do if accepted.
pub enum PendingConfirm {
    RemoveFolders(Vec<String>),
    DeleteTrack(TrackRef),
    Update(crate::update::Release),
}

/// Sidebar-only UI state.
#[derive(Default)]
pub struct SidebarState {
    pub selected: HashSet<String>,
    pub anchor: Option<String>,
    /// (folder id, text being typed)
    pub editing: Option<(String, String)>,
    pub editing_focus_requested: bool,
    pub drag: Option<crate::ui_sidebar::TreeDrag>,
    /// Last known scroll position, and one to apply next frame (auto-scroll).
    pub scroll_offset: f32,
    pub scroll_to: Option<f32>,
    pub hover_expand: Option<(String, Instant)>,
    /// Whether the sidebar was the last thing clicked (gates Delete / Ctrl+A).
    pub focused: bool,
}

pub struct DiscApp {
    pub settings: Settings,
    pub expanded: HashSet<String>,
    pub favorites: HashSet<String>,
    /// Tracks removed from Disc (not from disk), and the undo history for that.
    pub hidden: HashSet<String>,
    undo_hidden: Vec<Vec<String>>,

    pub folder_tracks: HashMap<String, Vec<TrackRef>>,
    pub missing: HashSet<String>,
    pub model: Model,
    model_dirty: bool,
    pub all_tracks: Vec<TrackRef>,
    pub view: Vec<TrackRef>,
    view_dirty: bool,

    /// "all" | "favorites" | a tree node key
    pub active: String,
    pub search: String,
    pub search_focus_requested: bool,

    pub library: Library,
    pub waves: Waveforms,
    pub audio: Option<Audio>,
    pub current: Option<TrackRef>,
    pub queue: Vec<String>,
    pub queue_index: Option<usize>,
    pub pinned: bool,
    pub muted_volume: Option<f32>,

    pub sb: SidebarState,
    /// The theme picker popup in the folder column footer.
    pub theme_menu_open: bool,
    /// Where that popup was last drawn (None while closed).
    pub theme_menu_rect: Option<egui::Rect>,
    /// (track id, text being typed)
    pub renaming: Option<(String, String)>,
    pub renaming_focus_requested: bool,
    pub confirm: Option<(Confirm, PendingConfirm)>,
    pub toast: Option<(String, Instant)>,

    settings_dirty_at: Option<Instant>,
    last_cache_save: Instant,
    /// Our own drag-out can land back on our window; ignore that "drop".
    pub ignore_drops_until: Instant,
    /// After a native drag-out, the button release happened inside Windows'
    /// drag loop where egui couldn't see it; feed one in next frame.
    pub inject_release: bool,
    pub updater: crate::update::Updater,
    /// Set once an update is in place: started after we save and exit.
    restart_into: Option<PathBuf>,
    rng: u64,
}

impl DiscApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));

        // First launch: bring over the old Disc Lite's setup if it exists.
        let (settings, imported) = match persist::load() {
            Some(s) => (s, false),
            None => match migrate::import_from_electron() {
                Some(s) => (s, true),
                None => (Settings::default(), false),
            },
        };
        if imported {
            persist::save(&settings);
        }
        theme::install(&ctx, &settings.theme);

        let audio = Audio::new().ok();
        if let Some(a) = &audio {
            a.set_volume(settings.volume);
        }

        let mut app = Self {
            expanded: settings.expanded.iter().cloned().collect(),
            favorites: settings.favorites.iter().cloned().collect(),
            hidden: settings.hidden.iter().cloned().collect(),
            undo_hidden: Vec::new(),
            settings,
            folder_tracks: HashMap::new(),
            missing: HashSet::new(),
            model: Model::default(),
            model_dirty: true,
            all_tracks: Vec::new(),
            view: Vec::new(),
            view_dirty: true,
            active: "all".into(),
            search: String::new(),
            search_focus_requested: false,
            library: Library::new(ctx.clone()),
            waves: Waveforms::new(ctx.clone(), persist::waveform_cache_path()),
            audio,
            current: None,
            queue: Vec::new(),
            queue_index: None,
            pinned: false,
            muted_volume: None,
            sb: SidebarState::default(),
            theme_menu_open: false,
            theme_menu_rect: None,
            renaming: None,
            renaming_focus_requested: false,
            confirm: None,
            toast: if imported {
                Some(("Imported your folders and favorites from the previous Disc Lite".into(), Instant::now()))
            } else {
                None
            },
            settings_dirty_at: None,
            last_cache_save: Instant::now(),
            ignore_drops_until: Instant::now(),
            inject_release: false,
            updater: crate::update::Updater::new(),
            restart_into: None,
            rng: seed(),
        };
        if app.audio.is_none() {
            app.show_toast("No audio output device found — playback is unavailable");
        }
        app.sync_folders();
        // Quietly look for a newer release; the footer shows an Update
        // button only if there is one.
        app.updater.check(&ctx);
        app
    }

    // --- Bookkeeping ------------------------------------------------------

    pub fn mark_settings_dirty(&mut self) {
        self.settings.expanded = self.expanded.iter().cloned().collect();
        self.settings.favorites = self.favorites.iter().cloned().collect();
        self.settings.hidden = { let mut h: Vec<String> = self.hidden.iter().cloned().collect(); h.sort(); h };
        self.settings_dirty_at = Some(Instant::now());
    }

    pub fn mark_view_dirty(&mut self) {
        self.view_dirty = true;
    }

    pub fn show_toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Watches + scans every folder, stops watching removed ones.
    fn sync_folders(&mut self) {
        let pairs: Vec<(String, String)> =
            self.settings.folders.iter().map(|f| (f.id.clone(), f.folder_path.clone())).collect();
        for id in self.library.sync_watches(&pairs) {
            if let Some(f) = self.settings.folders.iter().find(|f| f.id == id) {
                self.library.scan(&f.id, &f.folder_path);
            }
        }
        let live: HashSet<&String> = self.settings.folders.iter().map(|f| &f.id).collect();
        self.folder_tracks.retain(|id, _| live.contains(id));
        self.missing.retain(|id| live.contains(id));
        self.model_dirty = true;
    }

    fn rescan(&self, folder_id: &str) {
        if let Some(f) = self.settings.folders.iter().find(|f| f.id == folder_id) {
            self.library.scan(&f.id, &f.folder_path);
        }
    }

    fn drain_scans(&mut self) {
        while let Ok(ScanResult::Scanned { folder_id, tracks }) = self.library.results.try_recv() {
            if !self.settings.folders.iter().any(|f| f.id == folder_id) {
                continue;
            }
            match tracks {
                // Unreachable: keep the last-known tracks, just flag it.
                None => {
                    self.missing.insert(folder_id);
                }
                Some(t) => {
                    self.missing.remove(&folder_id);
                    self.folder_tracks.insert(folder_id, t);
                }
            }
            self.model_dirty = true;
        }
    }

    fn rebuild_if_needed(&mut self) {
        if self.model_dirty {
            self.model_dirty = false;
            // Removed-from-Disc tracks are filtered out before anything sees them.
            let filtered: HashMap<String, Vec<TrackRef>>;
            let tracks = if self.hidden.is_empty() {
                &self.folder_tracks
            } else {
                filtered = self
                    .folder_tracks
                    .iter()
                    .map(|(id, ts)| (id.clone(), ts.iter().filter(|t| !self.hidden.contains(&t.id)).cloned().collect()))
                    .collect();
                &filtered
            };
            self.model = tree::build_model(
                &self.settings.folders,
                tracks,
                &self.settings.placement,
                &self.settings.order,
            );
            let mut seen = HashSet::new();
            self.all_tracks.clear();
            for f in &self.settings.folders {
                for t in tracks.get(&f.id).into_iter().flatten() {
                    if seen.insert(t.id.clone()) {
                        self.all_tracks.push(t.clone());
                    }
                }
            }
            // The view's folder may have just disappeared.
            if !matches!(self.active.as_str(), "all" | "favorites") && !self.model.nodes.contains_key(&self.active) {
                let folder_id = parse_selection(&self.active).0.to_string();
                if !self.settings.folders.iter().any(|f| f.id == folder_id) {
                    self.active = "all".into();
                }
            }
            self.view_dirty = true;
        }
        if self.view_dirty {
            self.view_dirty = false;
            let q = self.search.trim().to_lowercase();
            self.view = if !q.is_empty() {
                // A search always spans the whole library.
                self.all_tracks.iter().filter(|t| t.file_name.to_lowercase().contains(&q)).cloned().collect()
            } else if self.active == "favorites" {
                self.all_tracks.iter().filter(|t| self.favorites.contains(&t.id)).cloned().collect()
            } else if self.active == "all" {
                self.all_tracks.clone()
            } else {
                self.model.node_tracks(&self.active)
            };
        }
    }

    pub fn track_by_id(&self, id: &str) -> Option<TrackRef> {
        self.all_tracks.iter().find(|t| t.id == id).cloned()
    }

    pub fn is_missing(&self, t: &TrackRef) -> bool {
        self.missing.contains(&t.folder_id)
    }

    /// The real directory a view points at (for importing/dropping files).
    pub fn active_dir(&self) -> Option<(PathBuf, String)> {
        if matches!(self.active.as_str(), "all" | "favorites") {
            return None;
        }
        let (folder_id, sub) = parse_selection(&self.active);
        let folder = self.settings.folders.iter().find(|f| f.id == folder_id)?;
        let mut dir = PathBuf::from(&folder.folder_path);
        for part in sub.split('/').filter(|s| !s.is_empty()) {
            dir.push(part);
        }
        let name = self.model.nodes.get(&self.active).map(|n| n.name.clone()).unwrap_or_else(|| folder.name.clone());
        Some((dir, name))
    }

    pub fn set_active(&mut self, key: &str) {
        if self.active != key {
            self.active = key.to_string();
            self.view_dirty = true;
        }
    }

    // --- Folders ------------------------------------------------------------

    pub fn add_folders(&mut self) {
        let Some(picked) = rfd::FileDialog::new().set_title("Add music folders").pick_folders() else { return };
        let existing: HashSet<String> =
            self.settings.folders.iter().map(|f| f.folder_path.to_lowercase()).collect();
        let mut fresh: Vec<Folder> = picked
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| !existing.contains(&p.to_lowercase()))
            .map(|p| Folder { id: new_folder_id(), name: folder_name_from_path(&p), folder_path: p })
            .collect();
        // Folder ids are time-based; make sure a batch can't collide.
        for (i, f) in fresh.iter_mut().enumerate() {
            f.id = format!("{}-{i}", f.id);
        }
        fresh.sort_by_key(|f| f.name.to_lowercase());
        if fresh.is_empty() {
            return;
        }
        if fresh.len() == 1 {
            self.active = fresh[0].id.clone();
        }
        self.settings.folders.extend(fresh);
        self.mark_settings_dirty();
        self.sync_folders();
    }

    pub fn remove_folders(&mut self, ids: &[String]) {
        let gone: HashSet<&String> = ids.iter().collect();
        let mentions_gone = |key: &str| !key.is_empty() && gone.contains(&parse_selection(key).0.to_string());
        self.settings.placement.retain(|k, v| !mentions_gone(k) && !mentions_gone(v));
        self.settings.order.retain(|k, _| !mentions_gone(k));
        for list in self.settings.order.values_mut() {
            list.retain(|k| !mentions_gone(k));
        }
        self.settings.folders.retain(|f| !gone.contains(&f.id));
        if mentions_gone(&self.active) {
            self.active = "all".into();
        }
        self.sb.selected.clear();
        self.mark_settings_dirty();
        self.sync_folders();
    }

    pub fn rename_folder(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(f) = self.settings.folders.iter_mut().find(|f| f.id == id) {
            f.name = name.to_string();
        }
        self.mark_settings_dirty();
        self.model_dirty = true;
    }

    pub fn move_nodes(&mut self, keys: &[String], parent: &str, before: Option<&str>) {
        if tree::move_nodes(&self.model, &mut self.settings.placement, &mut self.settings.order, keys, parent, before) {
            if !parent.is_empty() {
                self.expanded.insert(parent.to_string());
            }
            self.mark_settings_dirty();
            self.model_dirty = true;
        }
    }

    // --- Files ----------------------------------------------------------------

    /// Copies files into the folder currently being viewed, off the UI thread.
    pub fn import_into_active(&mut self, files: Vec<PathBuf>) {
        let files: Vec<PathBuf> = files.into_iter().filter(|p| library::is_importable(p)).collect();
        if files.is_empty() {
            return;
        }
        let Some((dir, name)) = self.active_dir() else {
            self.show_toast("Pick a folder on the left first, then drop files onto it");
            return;
        };
        let folder_id = parse_selection(&self.active).0.to_string();
        let n = files.len();
        let tx_rescan = {
            let f = self.settings.folders.iter().find(|f| f.id == folder_id).cloned();
            f.map(|f| (f.id, f.folder_path))
        };
        let lib_scan = self.library_scan_handle();
        std::thread::spawn(move || {
            library::copy_into(&dir, &files);
            if let (Some((id, path)), Some(scan)) = (tx_rescan, lib_scan) {
                scan(id, path);
            }
        });
        self.show_toast(format!("Adding {n} file{} to {name}", if n == 1 { "" } else { "s" }));
    }

    fn library_scan_handle(&self) -> Option<Box<dyn Fn(String, String) + Send>> {
        let tx = self.library.scan_sender();
        Some(Box::new(move |id, path| {
            let _ = tx.send((id, PathBuf::from(path)));
        }))
    }

    pub fn pick_and_import(&mut self) {
        if let Some(files) = rfd::FileDialog::new()
            .set_title("Import files")
            .add_filter("Audio / video", &["mp3", "wav", "ogg", "mov"])
            .pick_files()
        {
            self.import_into_active(files);
        }
    }

    pub fn rename_track(&mut self, track: &TrackRef, new_stem: &str) {
        match library::rename_track(&track.path, new_stem) {
            Ok(new_path) => {
                let new_id = new_path.to_string_lossy().into_owned();
                if new_id != track.id && self.favorites.remove(&track.id) {
                    self.favorites.insert(new_id);
                    self.mark_settings_dirty();
                }
                self.waves.invalidate(&track.id);
                self.rescan(&track.folder_id);
            }
            Err(e) => self.show_toast(format!("Couldn't rename: {e}")),
        }
    }

    pub fn delete_track(&mut self, track: &TrackRef) {
        if self.current.as_ref().is_some_and(|c| c.id == track.id) {
            if let Some(a) = &mut self.audio {
                a.pause();
                a.mark_unloaded();
            }
        }
        match library::delete_to_trash(&track.path) {
            Ok(()) => self.rescan(&track.folder_id),
            Err(e) => self.show_toast(format!("Couldn't delete: {e}")),
        }
    }

    /// Hides a track from Disc. The file stays where it is; Ctrl+Z undoes it.
    pub fn remove_track_from_disc(&mut self, track: &TrackRef) {
        if !self.hidden.insert(track.id.clone()) {
            return;
        }
        self.undo_hidden.push(vec![track.id.clone()]);
        self.mark_settings_dirty();
        self.model_dirty = true;
        self.show_toast(format!("Removed \"{}\" from Disc  ·  Ctrl+Z to undo", track.stem));
    }

    /// Brings back the most recently removed track(s).
    pub fn undo_remove(&mut self) {
        let Some(ids) = self.undo_hidden.pop() else { return };
        for id in &ids {
            self.hidden.remove(id);
        }
        self.mark_settings_dirty();
        self.model_dirty = true;
        let name = self
            .folder_tracks
            .values()
            .flatten()
            .find(|t| ids.contains(&t.id))
            .map(|t| t.stem.clone())
            .unwrap_or_else(|| "track".into());
        self.show_toast(format!("Restored \"{name}\""));
    }

    pub fn toggle_favorite(&mut self, id: &str) {
        if !self.favorites.remove(id) {
            self.favorites.insert(id.to_string());
        }
        self.mark_settings_dirty();
        if self.active == "favorites" {
            self.view_dirty = true;
        }
    }

    // --- Playback ---------------------------------------------------------------

    pub fn is_playing(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.is_active() && !a.is_paused()) && self.current.is_some()
    }

    pub fn current_duration(&self) -> f64 {
        let Some(t) = &self.current else { return 0.0 };
        self.audio
            .as_ref()
            .and_then(|a| a.duration)
            .or_else(|| self.waves.duration(&t.id))
            .unwrap_or(0.0)
    }

    pub fn position(&self) -> f64 {
        self.audio.as_ref().map(|a| a.position()).unwrap_or(0.0)
    }

    pub fn play_track(&mut self, track: &TrackRef, start_fraction: f64) {
        if track.is_video || self.is_missing(track) {
            return;
        }
        let known = self.waves.duration(&track.id);
        let Some(audio) = &mut self.audio else { return };
        match audio.load(&track.path, start_fraction, known) {
            Ok(()) => self.current = Some(track.clone()),
            Err(e) => self.show_toast(format!("Couldn't play {}: {e}", track.stem)),
        }
    }

    pub fn toggle_play(&mut self, track: &TrackRef, restart: bool) {
        let same = self.current.as_ref().is_some_and(|c| c.id == track.id);
        let Some(audio) = &mut self.audio else { return };
        if same && audio.is_active() {
            if restart {
                audio.restart();
            } else if audio.is_paused() {
                audio.play();
            } else {
                audio.pause();
            }
        } else {
            self.play_track(track, 0.0);
        }
    }

    pub fn seek_to(&mut self, track: &TrackRef, fraction: f64) {
        let same = self.current.as_ref().is_some_and(|c| c.id == track.id);
        if same && self.audio.as_ref().is_some_and(|a| a.is_active()) {
            if let Some(a) = &mut self.audio {
                if a.duration.is_none() {
                    a.duration = self.waves.duration(&track.id);
                }
                a.seek_fraction(fraction);
                a.play();
            }
        } else {
            self.play_track(track, fraction);
        }
    }

    pub fn shuffled_ids(&mut self, ids: &[String]) -> Vec<String> {
        let mut v = ids.to_vec();
        for i in (1..v.len()).rev() {
            let j = (next_rand(&mut self.rng) % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
        v
    }

    /// Starts a queue from whatever's in the list right now.
    pub fn play_all(&mut self, shuffle: bool) {
        let ids: Vec<String> = self.view.iter().filter(|t| !t.is_video).map(|t| t.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let mut ordered = if shuffle { self.shuffled_ids(&ids) } else { ids };
        // A fresh shuffle shouldn't open on the track already playing.
        if shuffle && ordered.len() > 1 && self.current.as_ref().is_some_and(|c| c.id == ordered[0]) {
            ordered.swap(0, 1);
        }
        self.queue = ordered;
        self.queue_index = Some(0);
        if let Some(t) = self.track_by_id(&self.queue[0]) {
            self.play_track(&t, 0.0);
        }
    }

    pub fn play_next(&mut self) {
        let Some(i) = self.queue_index else { return };
        if i + 1 < self.queue.len() {
            self.queue_index = Some(i + 1);
            if let Some(t) = self.track_by_id(&self.queue[i + 1]) {
                self.play_track(&t, 0.0);
            } else {
                self.play_next();
            }
            return;
        }
        // End of the queue: with Shuffle on, reshuffle and keep going.
        if self.settings.shuffle && !self.queue.is_empty() {
            let last = self.queue[i].clone();
            let ids = self.queue.clone();
            let mut next = self.shuffled_ids(&ids);
            if next.len() > 1 && next[0] == last {
                next.swap(0, 1);
            }
            self.queue = next;
            self.queue_index = Some(0);
            if let Some(t) = self.track_by_id(&self.queue[0]) {
                self.play_track(&t, 0.0);
            }
        } else {
            self.queue.clear();
            self.queue_index = None;
        }
    }

    pub fn play_prev(&mut self) {
        let Some(i) = self.queue_index else { return };
        if i == 0 {
            return;
        }
        self.queue_index = Some(i - 1);
        if let Some(t) = self.track_by_id(&self.queue[i - 1]) {
            self.play_track(&t, 0.0);
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.settings.shuffle = !self.settings.shuffle;
        self.mark_settings_dirty();
        self.show_toast(if self.settings.shuffle { "Shuffle on" } else { "Shuffle off" });
    }

    pub fn toggle_loop(&mut self) {
        self.settings.loop_one = !self.settings.loop_one;
        self.mark_settings_dirty();
        self.show_toast(if self.settings.loop_one { "Loop on — repeating this track" } else { "Loop off" });
    }

    pub fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.settings.volume = v;
        self.muted_volume = None;
        if let Some(a) = &self.audio {
            a.set_volume(v);
        }
        self.mark_settings_dirty();
    }

    pub fn toggle_mute(&mut self) {
        match self.muted_volume.take() {
            Some(v) => self.set_volume(v),
            None => {
                let v = self.settings.volume;
                self.set_volume(0.0);
                self.muted_volume = Some(if v > 0.0 { v } else { 0.8 });
            }
        }
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, id: &str) {
        theme::set_theme(ctx, id);
        self.settings.theme = theme::current().id.to_string();
        self.mark_settings_dirty();
    }

    pub fn toggle_pin(&mut self, ctx: &egui::Context) {
        self.pinned = !self.pinned;
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if self.pinned {
            egui::WindowLevel::AlwaysOnTop
        } else {
            egui::WindowLevel::Normal
        }));
    }

    // --- Per-frame -----------------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        // Ctrl+Z: undo the last "Remove from Disc" (text boxes keep their own undo).
        let undo = ctx.input(|i| !typing && i.modifiers.command && !i.modifiers.shift && i.key_pressed(egui::Key::Z));
        if undo {
            self.undo_remove();
        }
        let l_key = ctx.input(|i| !typing && !i.modifiers.command && i.key_pressed(egui::Key::L));
        if l_key {
            self.toggle_loop();
        }
        let (focus_search, space, left, right, up, down, s_key) = ctx.input(|i| {
            let ctrl_f = i.modifiers.command && i.key_pressed(egui::Key::F);
            (
                ctrl_f || (!typing && i.key_pressed(egui::Key::Slash)),
                !typing && i.key_pressed(egui::Key::Space),
                !typing && i.key_pressed(egui::Key::ArrowLeft),
                !typing && i.key_pressed(egui::Key::ArrowRight),
                !typing && i.key_pressed(egui::Key::ArrowUp),
                !typing && i.key_pressed(egui::Key::ArrowDown),
                !typing && !i.modifiers.command && i.key_pressed(egui::Key::S),
            )
        });
        if focus_search {
            self.search_focus_requested = true;
        }
        if space {
            if let Some(t) = self.current.clone() {
                self.toggle_play(&t, false);
            }
        }
        if left || right {
            if let Some(a) = &mut self.audio {
                a.seek_relative(if left { -5.0 } else { 5.0 });
            }
        }
        if up {
            self.set_volume(((self.settings.volume + 0.05) * 100.0).round() / 100.0);
        }
        if down {
            self.set_volume(((self.settings.volume - 0.05) * 100.0).round() / 100.0);
        }
        if s_key {
            self.toggle_shuffle();
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> =
            ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        if dropped.is_empty() || Instant::now() < self.ignore_drops_until {
            return;
        }
        self.import_into_active(dropped);
    }

    fn tick_playback(&mut self) {
        // A track that ran out on its own moves the queue along.
        let ended = self.current.is_some() && self.audio.as_ref().is_some_and(|a| a.finished());
        if ended {
            if let Some(a) = &mut self.audio {
                a.mark_unloaded();
            }
            // Loop: start the same track over instead of moving on.
            if self.settings.loop_one {
                if let Some(t) = self.current.clone() {
                    self.play_track(&t, 0.0);
                    return;
                }
            }
            if self.queue_index.is_some() {
                self.play_next();
            }
        }
    }

    fn handle_confirm(&mut self, ctx: &egui::Context) {
        let Some((c, _)) = &self.confirm else { return };
        match crate::widgets::confirm_modal(ctx, c) {
            ConfirmResult::Open => {}
            ConfirmResult::Cancelled => self.confirm = None,
            ConfirmResult::Confirmed => {
                if let Some((_, action)) = self.confirm.take() {
                    match action {
                        PendingConfirm::RemoveFolders(ids) => self.remove_folders(&ids),
                        PendingConfirm::DeleteTrack(t) => self.delete_track(&t),
                        PendingConfirm::Update(rel) => self.updater.install(ctx, rel),
                    }
                }
            }
        }
    }

    fn handle_update(&mut self, ctx: &egui::Context) {
        match self.updater.status() {
            crate::update::Status::Failed(msg) => {
                self.show_toast(msg);
                self.updater.reset();
            }
            crate::update::Status::Ready(exe) if self.restart_into.is_none() => {
                self.restart_into = Some(exe);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = &self.toast else { return };
        if at.elapsed() > Duration::from_secs(4) {
            self.toast = None;
            return;
        }
        let text = text.clone();
        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -70.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_PANEL_ALT())
                    .stroke(egui::Stroke::new(1.0, theme::BORDER()))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(14, 9))
                    .show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(text).font(theme::font(13.0)).color(theme::TEXT())).extend());
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn save_if_due(&mut self) {
        if let Some(at) = self.settings_dirty_at {
            if at.elapsed() > Duration::from_millis(800) {
                self.settings_dirty_at = None;
                persist::save(&self.settings);
            }
        }
        if self.last_cache_save.elapsed() > Duration::from_secs(30) {
            self.last_cache_save = Instant::now();
            self.waves.save_cache();
        }
    }
}

impl eframe::App for DiscApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::widgets::track_context_menu_clicks(&ctx);
        self.drain_scans();
        self.rebuild_if_needed();
        self.tick_playback();
        self.handle_keys(&ctx);
        self.handle_dropped_files(&ctx);
        self.rebuild_if_needed();

        crate::ui_player::show(self, ui, &ctx);
        crate::ui_sidebar::show(self, ui, &ctx);
        crate::ui_tracks::show(self, ui, &ctx, frame);

        self.handle_confirm(&ctx);
        self.handle_update(&ctx);
        self.draw_toast(&ctx);
        self.rebuild_if_needed();

        // Only repaint on a timer while something is actually moving.
        // Unfocused, progress updates drop to 4/sec so Disc stays out of
        // the way of whatever app is in front.
        if self.is_playing() {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            ctx.request_repaint_after(Duration::from_millis(if focused { 33 } else { 250 }));
        }
        self.save_if_due();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if std::mem::take(&mut self.inject_release) {
            let pos = raw
                .events
                .iter()
                .rev()
                .find_map(|e| if let egui::Event::PointerMoved(p) = e { Some(*p) } else { None })
                .unwrap_or(egui::Pos2::ZERO);
            raw.events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.expanded = self.expanded.iter().cloned().collect();
        self.settings.favorites = self.favorites.iter().cloned().collect();
        self.settings.hidden = { let mut h: Vec<String> = self.hidden.iter().cloned().collect(); h.sort(); h };
        persist::save(&self.settings);
        self.waves.save_cache();
        // Everything is saved; hand over to the freshly installed version.
        if let Some(exe) = self.restart_into.take() {
            let _ = std::process::Command::new(exe).spawn();
        }
    }
}

fn seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9E3779B97F4A7C15) | 1
}

fn next_rand(state: &mut u64) -> u64 {
    // xorshift64*
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(0x2545F4914F6CDD1D)
}
