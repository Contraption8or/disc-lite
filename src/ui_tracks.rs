//! The track list: toolbar (title, search, Import / Play / Shuffle) and one
//! row per track with its waveform. Click a waveform to seek; drag it out of
//! the window to drop the file into Premiere, Explorer, a DAW, ...

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Id, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::app::{DiscApp, PendingConfirm};
use crate::model::{TrackRef, format_duration, format_size};
use crate::waveform::Wave;
use crate::widgets::{self, ButtonKind, Confirm};
use crate::{icons, theme};

const ROW_H: f32 = 52.0;

enum Action {
    Toggle(TrackRef),
    PlayFromStart(TrackRef),
    Seek(TrackRef, f64),
    DragOut(TrackRef),
    Favorite(String),
    StartRename(TrackRef),
    CopyPath(TrackRef),
    Reveal(TrackRef),
    RemoveFromDisc(TrackRef),
    AskDelete(TrackRef),
}

fn view_title(app: &DiscApp) -> String {
    if !app.search.trim().is_empty() {
        return "Search".into();
    }
    match app.active.as_str() {
        "all" => "All Music".into(),
        "favorites" => "Favorites".into(),
        k => app.model.nodes.get(k).map(|n| n.name.clone()).unwrap_or_else(|| "Folder".into()),
    }
}

pub fn show(app: &mut DiscApp, ui: &mut egui::Ui, ctx: &egui::Context, frame: &eframe::Frame) {
    let panel_frame = egui::Frame::new().fill(theme::BG_APP()).inner_margin(egui::Margin { left: 20, right: 20, top: 14, bottom: 0 });
    egui::CentralPanel::default().frame(panel_frame).show(ui, |ui| {
        toolbar(app, ui, ctx);
        ui.add_space(12.0);
        let r = ui.available_rect_before_wrap();
        ui.painter().line_segment([pos2(r.left(), r.top()), pos2(r.right(), r.top())], Stroke::new(1.0, theme::GUIDE()));
        ui.add_space(6.0);
        list(app, ui, ctx, frame);
        drop_overlay(app, ui, ctx);
    });
}

fn toolbar(app: &mut DiscApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
    let title = view_title(app);
    let playable = app.view.iter().filter(|t| !t.is_video).count();
    let summary = {
        let n = app.view.len();
        let s = if n == 1 { "1 track".to_string() } else { format!("{n} tracks") };
        if app.search.trim().is_empty() { s } else { format!("{s} match \"{}\"", app.search.trim()) }
    };

    // Right side: buttons, then the search box to their left.
    let mut right = ui.new_child(
        egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right.spacing_mut().item_spacing = vec2(8.0, 0.0);
    let can_import = app.active_dir().is_some();
    if widgets::button(&mut right, "Play", ButtonKind::Primary, playable > 0, Some(&|p, c, col| icons::play(p, c, 11.0, col)))
        .on_hover_text(if app.settings.shuffle { "Play this list, shuffled (Shuffle is on)" } else { "Play this list in order" })
        .clicked()
    {
        let shuffle = app.settings.shuffle;
        app.play_all(shuffle);
    }
    let import = widgets::button(&mut right, "Import", ButtonKind::Ghost, can_import, None);
    let import = if can_import { import.on_hover_text("Copy audio files into this folder") } else { import.on_hover_text("Pick a folder on the left to import into it") };
    if import.clicked() {
        app.pick_and_import();
    }
    right.add_space(6.0);

    let search_w = 240.0f32.min((right.available_width() - 160.0).max(120.0));
    let (srect, _) = right.allocate_exact_size(vec2(search_w, 30.0), Sense::hover());
    let search_id = Id::new("search");
    let focused = ctx.memory(|m| m.has_focus(search_id));
    let p = ui.painter();
    p.rect_filled(srect, 7.0, theme::BG_PANEL_ALT());
    p.rect_stroke(srect, 7.0, Stroke::new(1.0, if focused { theme::TEXT3() } else { theme::BORDER() }), StrokeKind::Inside);
    icons::search(p, pos2(srect.left() + 15.0, srect.center().y), 15.0, theme::TEXT3());
    let edit_rect = Rect::from_min_max(pos2(srect.left() + 28.0, srect.top() + 3.0), pos2(srect.right() - 8.0, srect.bottom() - 3.0));
    let resp = right.put(
        edit_rect,
        egui::TextEdit::singleline(&mut app.search)
            .id(search_id)
            .frame(egui::Frame::NONE)
            .font(theme::font(13.0))
            .hint_text(egui::RichText::new("Search all music   Ctrl+F").color(theme::TEXT3()))
            .vertical_align(egui::Align::Center),
    );
    if app.search_focus_requested {
        app.search_focus_requested = false;
        resp.request_focus();
    }
    if resp.changed() {
        app.mark_view_dirty();
    }
    // TextEdit drops focus on Escape itself, so check lost_focus too.
    if (resp.has_focus() || resp.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.search.clear();
        app.mark_view_dirty();
        resp.surrender_focus();
    }

    // Left side: title + count, in whatever room is left.
    let left_w = (srect.left() - row.left() - 16.0).max(0.0);
    let p = ui.painter();
    let g = widgets::elided(p, &title, theme::font_semibold(20.0), theme::TEXT(), left_w);
    let tw = g.size().x;
    p.galley(pos2(row.left(), row.center().y - g.size().y / 2.0), g, theme::TEXT());
    let room = left_w - tw - 12.0;
    if room > 40.0 {
        let g = widgets::elided(p, &summary, theme::font(12.5), theme::TEXT3(), room);
        p.galley(pos2(row.left() + tw + 12.0, row.center().y - g.size().y / 2.0 + 2.0), g, theme::TEXT3());
    }
}

fn list(app: &mut DiscApp, ui: &mut egui::Ui, ctx: &egui::Context, frame: &eframe::Frame) {
    if app.view.is_empty() {
        empty(app, ui);
        return;
    }
    let mut actions: Vec<Action> = Vec::new();
    let n = app.view.len();
    let current_id = app.current.as_ref().map(|t| t.id.clone());
    let playing = app.is_playing();
    let progress = {
        let d = app.current_duration();
        if d > 0.0 { (app.position() / d).clamp(0.0, 1.0) as f32 } else { 0.0 }
    };

    egui::ScrollArea::vertical().id_salt("tracks").auto_shrink([false, false]).show_rows(ui, ROW_H, n, |ui, range| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let w = ui.available_width();
        let rows: Vec<TrackRef> = app.view[range].to_vec();
        for t in rows {
            let (rect, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
            let row_resp = ui.interact(rect, Id::new(("track", &t.id)), Sense::click());
            let is_current = current_id.as_deref() == Some(t.id.as_str());
            let missing = app.is_missing(&t);
            // The row counts as hovered even when the pointer is over one of
            // its own buttons or its waveform.
            let hovered = ui.rect_contains_pointer(rect);
            let renaming = app.renaming.as_ref().is_some_and(|(id, _)| *id == t.id);
            let inner = rect.shrink2(vec2(0.0, 3.0));

            {
                let p = ui.painter();
                if is_current {
                    p.rect_filled(inner, 8.0, theme::ROW_CURRENT());
                } else if hovered {
                    p.rect_filled(inner, 8.0, theme::ROW_HOVER());
                }
            }

            // Play button.
            let pc = pos2(inner.left() + 22.0, inner.center().y);
            let prect = Rect::from_center_size(pc, vec2(30.0, 30.0));
            let can_play = !t.is_video && !missing;
            let presp = ui.interact(prect, Id::new(("track-play", &t.id)), if can_play { Sense::click() } else { Sense::hover() });
            crate::widgets::hand(&presp, can_play);
            {
                let p = ui.painter();
                if t.is_video {
                    icons::video(p, pc, 18.0, theme::TEXT3());
                } else {
                    let fill = if is_current { theme::ACCENT() } else if presp.hovered() { theme::BG_ACTIVE() } else { theme::BG_HOVER() };
                    p.circle_filled(pc, 15.0, if missing { theme::BG_PANEL_ALT() } else { fill });
                    let col = if is_current { theme::ACCENT_TEXT() } else if missing { theme::TEXT3() } else { theme::TEXT() };
                    if is_current && playing {
                        icons::pause(p, pc, 11.0, col);
                    } else {
                        icons::play(p, pc + vec2(1.0, 0.0), 11.0, col);
                    }
                }
            }
            if can_play && presp.clicked() {
                actions.push(Action::Toggle(t.clone()));
            }

            // Layout: [play] [name / meta] [waveform] [duration] [heart]
            let info_left = inner.left() + 48.0;
            let info_w = (inner.width() * 0.30).clamp(200.0, 420.0);
            let heart_c = pos2(inner.right() - 20.0, inner.center().y);
            let dur_right = heart_c.x - 22.0;
            let wave = Rect::from_min_max(
                pos2(info_left + info_w + 20.0, inner.top() + 10.0),
                pos2(dur_right - 58.0, inner.bottom() - 10.0),
            );

            if renaming {
                let er = Rect::from_min_size(pos2(info_left - 4.0, inner.top() + 4.0), vec2(info_w, 24.0));
                let (_, text) = app.renaming.as_mut().unwrap();
                let r = ui.put(
                    er,
                    egui::TextEdit::singleline(text).id(Id::new("track-rename")).font(theme::font_semibold(13.5)).margin(egui::Margin::symmetric(4, 2)),
                );
                if !app.renaming_focus_requested {
                    app.renaming_focus_requested = true;
                    r.request_focus();
                }
                let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if esc {
                    app.renaming = None;
                } else if enter || r.lost_focus() {
                    if let Some((_, name)) = app.renaming.take() {
                        if name.trim() != t.stem {
                            app.rename_track(&t, &name);
                        }
                    }
                }
            } else {
                let p = ui.painter();
                let col = if missing { theme::TEXT3() } else { theme::TEXT() };
                let g = widgets::elided(p, &t.stem, theme::font_semibold(13.5), col, info_w);
                p.galley(pos2(info_left, inner.center().y - 1.0 - g.size().y), g, col);
            }
            {
                let mut meta: Vec<String> = Vec::new();
                if missing {
                    meta.push("Folder not found".into());
                }
                if let Some(loc) = location(app, &t) {
                    meta.push(loc);
                }
                meta.push(format_size(t.size));
                let p = ui.painter();
                let col = if missing { theme::DANGER() } else { theme::TEXT3() };
                let g = widgets::elided(p, &meta.join("  ·  "), theme::font(12.0), col, info_w);
                p.galley(pos2(info_left, inner.center().y + 2.0), g, col);
            }
            // Duration, right-aligned in its own column.
            if let Some(d) = app.waves.duration(&t.id) {
                ui.painter().text(
                    pos2(dur_right, inner.center().y),
                    Align2::RIGHT_CENTER,
                    format_duration(if is_current { (d * progress as f64).max(0.0) } else { d }),
                    theme::font(12.5),
                    if is_current { theme::TEXT() } else { theme::TEXT3() },
                );
            }

            // Waveform: click to seek, drag out to export.
            if wave.width() > 20.0 {
                let wresp = ui.interact(wave, Id::new(("track-wave", &t.id)), Sense::click_and_drag());
                crate::widgets::hand(&wresp, can_play || t.is_video);
                let hover_x = wresp.hover_pos().map(|p| p.x);
                let p = ui.painter();
                let played_x = if is_current { wave.left() + wave.width() * progress } else { wave.left() };
                let base = if hovered || is_current { theme::WAVEFORM_HOVER() } else { theme::WAVEFORM() };
                if t.is_video {
                    p.text(wave.left_center(), Align2::LEFT_CENTER, "Video — drag to export", theme::font(12.0), theme::TEXT3());
                } else {
                    if let Wave::Ready(peaks) = app.waves.get(&t.id, &t.path) {
                        draw_wave(p, wave, &peaks, played_x, base, hover_x.filter(|_| can_play));
                    } else {
                        // Still decoding: just the center line.
                        let y = wave.center().y;
                        p.line_segment([pos2(wave.left(), y), pos2(wave.right(), y)], Stroke::new(1.0, theme::WAVEFORM_BASE()));
                        if is_current {
                            p.line_segment([pos2(wave.left(), y), pos2(played_x, y)], Stroke::new(1.0, theme::WAVE_PLAYED()));
                        }
                    }
                }
                if let Some(x) = hover_x.filter(|_| can_play) {
                    p.line_segment([pos2(x, wave.top() - 2.0), pos2(x, wave.bottom() + 2.0)], Stroke::new(1.0, theme::TEXT()));
                }
                if wresp.drag_started() {
                    actions.push(Action::DragOut(t.clone()));
                } else if wresp.clicked() && can_play {
                    if let Some(x) = wresp.interact_pointer_pos().map(|p| p.x) {
                        actions.push(Action::Seek(t.clone(), ((x - wave.left()) / wave.width()).clamp(0.0, 1.0) as f64));
                    }
                }
                let _ = wresp;
            }

            // Favorite.
            let fav = app.favorites.contains(&t.id);
            let hrect = Rect::from_center_size(heart_c, vec2(28.0, 28.0));
            let hresp = ui.interact(hrect, Id::new(("track-fav", &t.id)), Sense::click());
            crate::widgets::hand(&hresp, true);
            if fav || hovered || hresp.hovered() {
                let col = if fav { theme::TEXT() } else if hresp.hovered() { theme::TEXT2() } else { theme::TEXT3() };
                icons::heart(ui.painter(), heart_c, 15.0, col, fav);
            }
            if hresp.on_hover_text(if fav { "Remove from Favorites" } else { "Add to Favorites" }).clicked() {
                actions.push(Action::Favorite(t.id.clone()));
            }

            if row_resp.double_clicked() && can_play {
                actions.push(Action::PlayFromStart(t.clone()));
            }
            let tc = t.clone();
            crate::widgets::context_menu(&row_resp, |ui| {
                ui.set_min_width(180.0);
                if ui.button("Rename").clicked() {
                    actions.push(Action::StartRename(tc.clone()));
                    ui.close();
                }
                if ui.button("Copy File Path").clicked() {
                    actions.push(Action::CopyPath(tc.clone()));
                    ui.close();
                }
                if ui.button("Reveal in Explorer").clicked() {
                    actions.push(Action::Reveal(tc.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button("Remove from Disc").on_hover_text("Hide it from Disc. The file stays on disk. Ctrl+Z to undo.").clicked() {
                    actions.push(Action::RemoveFromDisc(tc.clone()));
                    ui.close();
                }
                if ui.button(egui::RichText::new("Delete Song…").color(theme::DANGER())).clicked() {
                    actions.push(Action::AskDelete(tc.clone()));
                    ui.close();
                }
            });
        }
    });

    for a in actions {
        match a {
            Action::Toggle(t) => {
                // Clicking play in a list starts a queue from that point.
                start_queue_at(app, &t);
                app.toggle_play(&t, false);
            }
            Action::PlayFromStart(t) => {
                start_queue_at(app, &t);
                app.play_track(&t, 0.0);
            }
            Action::Seek(t, f) => {
                if app.current.as_ref().is_none_or(|c| c.id != t.id) {
                    start_queue_at(app, &t);
                }
                app.seek_to(&t, f);
            }
            Action::DragOut(t) => {
                if let Err(e) = crate::dragout::start_file_drag(frame, &t.path) {
                    app.show_toast(format!("Couldn't start the drag: {e}"));
                }
                // Windows ran its own drag loop; egui never saw the release.
                app.inject_release = true;
                app.ignore_drops_until = Instant::now() + Duration::from_millis(1500);
            }
            Action::Favorite(id) => app.toggle_favorite(&id),
            Action::StartRename(t) => {
                app.renaming = Some((t.id.clone(), t.stem.clone()));
                app.renaming_focus_requested = false;
            }
            Action::CopyPath(t) => {
                ctx.copy_text(t.path.to_string_lossy().into_owned());
                app.show_toast("Copied file path");
            }
            Action::Reveal(t) => crate::library::reveal_in_explorer(&t.path),
            Action::RemoveFromDisc(t) => app.remove_track_from_disc(&t),
            Action::AskDelete(t) => {
                app.confirm = Some((
                    Confirm {
                        title: format!("Delete \"{}\"?", t.stem),
                        message: "The file will be moved to the Recycle Bin.".into(),
                        confirm_label: "Delete".into(),
                    },
                    PendingConfirm::DeleteTrack(t),
                ));
            }
        }
    }
}

/// Makes the visible list the queue, positioned at `t` (keeps the current
/// queue if `t` is already in it and it was built from this same list).
fn start_queue_at(app: &mut DiscApp, t: &TrackRef) {
    let ids: Vec<String> = app.view.iter().filter(|x| !x.is_video).map(|x| x.id.clone()).collect();
    let same_list = app.queue.len() == ids.len() && {
        let mut a = app.queue.clone();
        let mut b = ids.clone();
        a.sort();
        b.sort();
        a == b
    };
    if same_list {
        if let Some(i) = app.queue.iter().position(|id| *id == t.id) {
            app.queue_index = Some(i);
            return;
        }
    }
    if app.settings.shuffle {
        let rest: Vec<String> = ids.into_iter().filter(|id| *id != t.id).collect();
        let mut q = vec![t.id.clone()];
        q.extend(app.shuffled_ids(&rest));
        app.queue = q;
        app.queue_index = Some(0);
    } else {
        app.queue_index = ids.iter().position(|id| *id == t.id);
        app.queue = ids;
    }
}

/// Where a track lives, relative to the folder being viewed. None when it
/// sits directly in that folder (so every row doesn't repeat the same path).
fn location(app: &DiscApp, t: &TrackRef) -> Option<String> {
    let folder_name = app.settings.folders.iter().find(|f| f.id == t.folder_id).map(|f| f.name.as_str()).unwrap_or("");
    let full = if t.rel_dir.is_empty() { folder_name.to_string() } else { format!("{folder_name} / {}", t.rel_dir.replace('/', " / ")) };
    if !app.search.trim().is_empty() || matches!(app.active.as_str(), "all" | "favorites") {
        return Some(full);
    }
    let (fid, rel) = crate::tree::parse_selection(&app.active);
    if fid != t.folder_id {
        return Some(full);
    }
    if t.rel_dir == rel {
        return None;
    }
    let prefix = if rel.is_empty() { String::new() } else { format!("{rel}/") };
    match t.rel_dir.strip_prefix(&prefix) {
        Some(rest) if !rest.is_empty() => Some(rest.replace('/', " / ")),
        _ => Some(full),
    }
}

/// Level of `data` over the fraction range [a, b): the loudest bucket when
/// the range spans several, a linear blend when it's narrower than one.
fn sample(data: &[f32], a: f32, b: f32) -> f32 {
    let n = data.len();
    let (fa, fb) = (a * n as f32, b * n as f32);
    if fb - fa < 1.0 {
        let x = ((fa + fb) / 2.0 - 0.5).clamp(0.0, (n - 1) as f32);
        let i = x.floor() as usize;
        let j = (i + 1).min(n - 1);
        let t = x - i as f32;
        return data[i] * (1.0 - t) + data[j] * t;
    }
    let i = (fa.floor() as usize).min(n - 1);
    let j = (fb.ceil() as usize).clamp(i + 1, n);
    data[i..j].iter().copied().fold(0.0, f32::max)
}

/// A continuous filled waveform, one column per screen pixel: the left
/// channel rises above the center line, the right channel hangs below it,
/// so a track panned to one side looks lopsided.
fn draw_wave(p: &egui::Painter, r: Rect, peaks: &crate::waveform::Peaks, played_x: f32, base: egui::Color32, hover_x: Option<f32>) {
    let ppp = p.ctx().pixels_per_point();
    let n = ((r.width() * ppp).floor() as usize).max(1);
    let col_w = r.width() / n as f32;
    let mid = r.center().y;
    let half = r.height() / 2.0;
    // Normalize so quiet tracks still read, without blowing up near-silence.
    let max = peaks.left.iter().chain(&peaks.right).copied().fold(0.0f32, f32::max);
    let gain = if max > 0.0 { (1.0 / max).min(3.0) } else { 1.0 };
    // Silence still shows as a hairline, like an editor's timeline.
    let min_h = 0.5 / ppp;

    let mut mesh = egui::Mesh::default();
    mesh.reserve_vertices(n * 4);
    mesh.reserve_triangles(n * 2);
    for c in 0..n {
        let (a, b) = (c as f32 / n as f32, (c + 1) as f32 / n as f32);
        let up = (sample(&peaks.left, a, b) * gain).min(1.0) * half;
        let down = (sample(&peaks.right, a, b) * gain).min(1.0) * half;
        let x0 = r.left() + c as f32 * col_w;
        let x1 = x0 + col_w;
        let col = if x1 <= played_x {
            theme::WAVE_PLAYED()
        } else if hover_x.is_some_and(|hx| x1 <= hx) {
            theme::WAVEFORM_PREVIEW()
        } else {
            base
        };
        mesh.add_colored_rect(Rect::from_min_max(pos2(x0, mid - up.max(min_h)), pos2(x1, mid + down.max(min_h))), col);
    }
    p.add(egui::Shape::mesh(mesh));
}

fn empty(app: &mut DiscApp, ui: &mut egui::Ui) {
    if app.settings.folders.is_empty() {
        if widgets::empty_state(
            ui,
            |p, c, col| icons::folder(p, c, 26.0, col),
            "Add your music",
            "Point Disc at the folders where your music lives. It reads them in place — nothing is copied or moved.",
            Some("Add Folder"),
        ) {
            app.add_folders();
        }
    } else if !app.search.trim().is_empty() {
        widgets::empty_state(ui, |p, c, col| icons::search(p, c, 26.0, col), "No matches", "Nothing in your library matches that search.", None);
    } else if app.active == "favorites" {
        widgets::empty_state(
            ui,
            |p, c, col| icons::heart(p, c, 24.0, col, false),
            "No favorites yet",
            "Hover a track and click its heart to keep it here.",
            None,
        );
    } else if app.active == "all" {
        widgets::empty_state(ui, |p, c, col| icons::music_note(p, c, 26.0, col), "No tracks found", "Your folders don't have any mp3, wav, ogg, or mov files yet.", None);
    } else if widgets::empty_state(
        ui,
        |p, c, col| icons::music_note(p, c, 26.0, col),
        "Nothing here yet",
        "Drop audio files onto this window to add them to this folder.",
        Some("Import Files"),
    ) {
        app.pick_and_import();
    }
}

fn drop_overlay(app: &DiscApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
    if !hovering || Instant::now() < app.ignore_drops_until {
        return;
    }
    let r = ui.max_rect().shrink(10.0);
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop-overlay")));
    p.rect_filled(r, 12.0, egui::Color32::from_black_alpha(200));
    p.rect_stroke(r, 12.0, Stroke::new(1.5, theme::TEXT2()), StrokeKind::Inside);
    let text = match app.active_dir() {
        Some((_, name)) => format!("Drop to add to {name}"),
        None => "Pick a folder on the left first".into(),
    };
    p.text(r.center(), Align2::CENTER_CENTER, text, theme::font_semibold(18.0), theme::TEXT());
}
