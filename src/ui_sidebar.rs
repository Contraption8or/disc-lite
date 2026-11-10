//! The fixed-width column on the left: All Music, Favorites, and the folder
//! tree (VS Code style) with multi-select, rename, remove, and drag-to-move.
//! Moving and removing only ever change Disc's own list â€” nothing on disk.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Id, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::app::{DiscApp, PendingConfirm};
use crate::tree::NodeKind;
use crate::widgets::{self, Confirm};
use crate::{icons, theme};

pub const WIDTH: f32 = 260.0;
const ROW_H: f32 = 26.0;
const INDENT: f32 = 14.0;
const HOVER_EXPAND: Duration = Duration::from_millis(650);

pub struct TreeDrag {
    pub keys: Vec<String>,
    pub label: String,
    pub target: Option<DropTarget>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum DropTarget {
    Before(String),
    Into(String),
    After(String),
    /// Below every row: the end of the top level.
    End,
}

struct Row {
    key: String,
    name: String,
    kind: NodeKind,
    depth: usize,
    has_children: bool,
    expanded: bool,
    missing: bool,
}

enum Action {
    Select { key: String, shift: bool, ctrl: bool },
    Toggle(String),
    StartRename(String),
    AskRemove(Vec<String>),
    Reveal(String),
}

fn flatten(app: &DiscApp) -> Vec<Row> {
    fn walk(app: &DiscApp, parent: &str, depth: usize, out: &mut Vec<Row>) {
        for key in app.model.children(parent) {
            let Some(n) = app.model.nodes.get(key) else { continue };
            let has_children = !app.model.children(key).is_empty();
            let expanded = has_children && app.expanded.contains(key);
            out.push(Row {
                key: key.clone(),
                name: n.name.clone(),
                kind: n.kind,
                depth,
                has_children,
                expanded,
                missing: app.missing.contains(&n.folder_id),
            });
            if expanded {
                walk(app, key, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(app, "", 0, &mut out);
    out
}

/// The folder ids (top-level added folders) among `keys`.
fn removable(app: &DiscApp, keys: &[String]) -> Vec<String> {
    keys.iter()
        .filter(|k| app.model.nodes.get(*k).is_some_and(|n| n.kind == NodeKind::Folder))
        .cloned()
        .collect()
}

fn ask_remove(app: &mut DiscApp, ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    let (title, message) = if ids.len() == 1 {
        let name = app.settings.folders.iter().find(|f| f.id == ids[0]).map(|f| f.name.clone()).unwrap_or_default();
        (
            format!("Remove \"{name}\" from Disc?"),
            "The folder and its files stay exactly where they are on disk â€” this only takes it off Disc's list.".to_string(),
        )
    } else {
        (
            format!("Remove {} folders from Disc?", ids.len()),
            "The folders and their files stay exactly where they are on disk â€” this only takes them off Disc's list."
                .to_string(),
        )
    };
    app.confirm = Some((Confirm { title, message, confirm_label: "Remove".into() }, PendingConfirm::RemoveFolders(ids)));
}

/// Turns a drop target into (new parent, insert-before) for `move_nodes`.
fn resolve(app: &DiscApp, target: &DropTarget) -> (String, Option<String>) {
    match target {
        DropTarget::End => (String::new(), None),
        DropTarget::Into(k) => (k.clone(), None),
        DropTarget::Before(k) => (app.model.eff_parent.get(k).cloned().unwrap_or_default(), Some(k.clone())),
        DropTarget::After(k) => {
            let kids = app.model.children(k);
            if app.expanded.contains(k) && !kids.is_empty() {
                return (k.clone(), Some(kids[0].clone()));
            }
            let parent = app.model.eff_parent.get(k).cloned().unwrap_or_default();
            let sibs = app.model.children(&parent);
            let next = sibs.iter().position(|s| s == k).and_then(|i| sibs.get(i + 1)).cloned();
            (parent, next)
        }
    }
}

fn target_valid(app: &DiscApp, keys: &[String], target: &DropTarget) -> bool {
    if let DropTarget::Into(k) = target {
        if keys.contains(k) {
            return false;
        }
    }
    let (parent, _) = resolve(app, target);
    parent.is_empty() || !keys.iter().any(|k| app.model.is_within(&parent, k))
}

pub fn show(app: &mut DiscApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let frame = egui::Frame::new().fill(theme::BG_PANEL()).inner_margin(egui::Margin::symmetric(8, 10));
    let panel = egui::Panel::left("sidebar").exact_size(WIDTH).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        footer(app, ui);
        sidebar_body(app, ui, ctx);
    });
    let rect = panel.response.rect;
    // (The divider between this column and the track list is drawn by the
    // track list itself, along with its rounded corner.)

    // Keyboard focus follows the last click: inside the sidebar or not.
    let pressed_at = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
    if let Some(p) = pressed_at {
        app.sb.focused = rect.contains(p);
    }
}

fn nav_row(ui: &mut egui::Ui, id: &str, label: &str, count: usize, active: bool, icon: impl FnOnce(&egui::Painter, egui::Pos2, egui::Color32)) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let resp = ui.interact(rect, Id::new(("nav", id)), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 6.0, theme::BG_ACTIVE());
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, theme::BG_HOVER());
    }
    let col = if active { theme::TEXT() } else { theme::TEXT2() };
    icon(p, pos2(rect.left() + 16.0, rect.center().y), col);
    p.text(pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, theme::font_semibold(13.0), col);
    p.text(
        pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        count.to_string(),
        theme::font(12.0),
        theme::TEXT3(),
    );
    resp
}

fn sidebar_body(app: &mut DiscApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.spacing_mut().item_spacing = vec2(0.0, 2.0);

    let total = app.all_tracks.len();
    let favs = app.all_tracks.iter().filter(|t| app.favorites.contains(&t.id)).count();
    if nav_row(ui, "all", "All Music", total, app.active == "all", |p, c, col| icons::music_note(p, c, 16.0, col)).clicked() {
        app.set_active("all");
        app.sb.selected.clear();
    }
    if nav_row(ui, "fav", "Favorites", favs, app.active == "favorites", |p, c, col| icons::heart(p, c, 13.0, col, false))
        .clicked()
    {
        app.set_active("favorites");
        app.sb.selected.clear();
    }
    // Only there once something has been removed from Disc.
    let removed = app.removed_tracks.len();
    if removed > 0
        && nav_row(ui, "removed", "Removed", removed, app.active == "removed", |p, c, col| icons::eye_off(p, c, 15.0, col))
            .on_hover_text("Tracks you removed from Disc. Their files are still on disk.")
            .clicked()
    {
        app.set_active("removed");
        app.sb.selected.clear();
    }

    ui.add_space(14.0);
    // Section heading with the add button.
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(
        pos2(head.left() + 8.0, head.center().y),
        Align2::LEFT_CENTER,
        "FOLDERS",
        theme::font_semibold(11.0),
        theme::TEXT3(),
    );
    let add_r = Rect::from_center_size(pos2(head.right() - 12.0, head.center().y), vec2(22.0, 22.0));
    let add = ui.interact(add_r, Id::new("add-folder"), Sense::click());
    crate::widgets::hand(&add, true);
    if add.hovered() {
        ui.painter().rect_filled(add_r, 5.0, theme::BG_HOVER());
    }
    icons::folder_plus(ui.painter(), add_r.center(), 16.0, if add.hovered() { theme::TEXT() } else { theme::TEXT2() });
    if add.on_hover_text("Add folders").clicked() {
        app.add_folders();
    }
    ui.add_space(4.0);

    let rows = flatten(app);
    let multi = app.sb.selected.len() > 1;
    let bar_h = if multi { 44.0 } else { 0.0 };

    if rows.is_empty() {
        let avail = ui.available_rect_before_wrap();
        let mut job = egui::text::LayoutJob::simple(
            "Add a folder of music to get started. Disc reads it where it is â€” nothing gets copied or moved."
                .to_string(),
            theme::font(12.5),
            theme::TEXT3(),
            avail.width() - 24.0,
        );
        job.halign = egui::Align::Center;
        let g = ui.painter().layout_job(job);
        ui.painter().galley(pos2(avail.center().x, avail.top() + 24.0), g, theme::TEXT3());
        return;
    }

    let (shift, ctrl) = ctx.input(|i| (i.modifiers.shift, i.modifiers.command));
    let primary_down = ctx.input(|i| i.pointer.primary_down());
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    let dragging = app.sb.drag.is_some();
    // The theme menu floats over the tree; clicks on it must not reach the
    // folder rows underneath.
    let over_menu = app.theme_menu_rect.zip(pointer).is_some_and(|(r, p)| r.contains(p));
    let mut actions: Vec<Action> = Vec::new();
    let mut row_rects: Vec<(usize, Rect)> = Vec::with_capacity(rows.len());

    let mut area = egui::ScrollArea::vertical()
        .id_salt("tree")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - bar_h).max(40.0));
    if let Some(y) = app.sb.scroll_to.take() {
        area = area.vertical_scroll_offset(y);
    }
    let out = area.show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let w = ui.available_width();
        for (i, row) in rows.iter().enumerate() {
            let (rect, _) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::hover());
            row_rects.push((i, rect));
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let resp = ui.interact(rect, Id::new(("row", &row.key)), Sense::click_and_drag());
            let p = ui.painter();
            let is_active = app.active == row.key;
            let is_selected = app.sb.selected.contains(&row.key);
            let editing = app.sb.editing.as_ref().is_some_and(|(id, _)| *id == row.key);

            if is_active {
                p.rect_filled(rect, 5.0, theme::BG_ACTIVE());
            } else if is_selected {
                p.rect_filled(rect, 5.0, theme::BG_HOVER());
            } else if ui.rect_contains_pointer(rect) && !dragging {
                p.rect_filled(rect, 5.0, theme::BG_PANEL_ALT());
            }
            if is_selected && multi {
                p.rect_stroke(rect, 5.0, Stroke::new(1.0, theme::TEXT3()), StrokeKind::Inside);
            }

            // Indent guides.
            for d in 0..row.depth {
                let gx = rect.left() + 14.0 + d as f32 * INDENT;
                p.line_segment([pos2(gx, rect.top()), pos2(gx, rect.bottom())], Stroke::new(1.0, theme::GUIDE()));
            }
            let x0 = rect.left() + 8.0 + row.depth as f32 * INDENT;
            let chev_r = Rect::from_center_size(pos2(x0 + 6.0, rect.center().y), vec2(16.0, ROW_H));
            if row.has_children {
                icons::chevron(p, chev_r.center(), 14.0, theme::TEXT3(), row.expanded);
            }
            let mut name_x = x0 + 18.0;
            if row.missing {
                icons::warning(p, pos2(name_x + 6.0, rect.center().y), 13.0, theme::DANGER());
                name_x += 18.0;
            }

            // Not resp.hovered(): that goes false while the pointer is over
            // the row's own Ã— button, which would make the button vanish.
            let hovered_row = ui.rect_contains_pointer(rect) && !dragging;
            let show_x = hovered_row && row.kind == NodeKind::Folder && !editing;
            let right_pad = if show_x { 28.0 } else { 8.0 };

            if editing {
                let edit_rect = Rect::from_min_max(pos2(name_x - 4.0, rect.top() + 2.0), pos2(rect.right() - 4.0, rect.bottom() - 2.0));
                let (_, text) = app.sb.editing.as_mut().unwrap();
                let te = egui::TextEdit::singleline(text)
                    .font(theme::font(13.0))
                    .margin(egui::Margin::symmetric(4, 1))
                    .id(Id::new("folder-rename"));
                let r = ui.put(edit_rect, te);
                if !app.sb.editing_focus_requested {
                    app.sb.editing_focus_requested = true;
                    r.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, r.id) {
                        let len = app.sb.editing.as_ref().map(|(_, t)| t.chars().count()).unwrap_or(0);
                        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(len),
                        )));
                        state.store(ctx, r.id);
                    }
                }
                let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if esc {
                    app.sb.editing = None;
                } else if enter || r.lost_focus() {
                    if let Some((id, text)) = app.sb.editing.take() {
                        app.rename_folder(&id, &text);
                    }
                }
            } else {
                let col = if row.missing {
                    theme::TEXT3()
                } else if is_active || is_selected {
                    theme::TEXT()
                } else {
                    theme::TEXT2()
                };
                let font = if row.kind == NodeKind::Folder { theme::font_semibold(13.0) } else { theme::font(13.0) };
                let g = widgets::elided(p, &row.name, font, col, rect.right() - right_pad - name_x);
                p.galley(pos2(name_x, rect.center().y - g.size().y / 2.0), g, col);
            }

            if dragging || editing || over_menu {
                continue;
            }

            // Remove (Ã—) on hover â€” only Disc's list, never the files.
            let mut consumed = false;
            if show_x {
                let xr = Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), vec2(20.0, 20.0));
                let xresp = ui.interact(xr, Id::new(("row-x", &row.key)), Sense::click());
                crate::widgets::hand(&xresp, true);
                let p = ui.painter();
                if xresp.hovered() {
                    p.rect_filled(xr, 4.0, theme::BG_ACTIVE());
                }
                let c = xr.center();
                let d = 3.5;
                let st = Stroke::new(1.3, if xresp.hovered() { theme::TEXT() } else { theme::TEXT3() });
                p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], st);
                p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], st);
                if xresp.on_hover_text("Remove from Disc (files stay on disk)").clicked() {
                    let keys = if is_selected && multi {
                        app.sb.selected.iter().cloned().collect()
                    } else {
                        vec![row.key.clone()]
                    };
                    actions.push(Action::AskRemove(keys));
                    consumed = true;
                }
            }
            if row.has_children {
                let cresp = ui.interact(chev_r, Id::new(("row-chev", &row.key)), Sense::click());
                crate::widgets::hand(&cresp, true);
                if cresp.clicked() {
                    actions.push(Action::Toggle(row.key.clone()));
                    consumed = true;
                }
            }

            if !consumed {
                if resp.double_clicked() && row.kind == NodeKind::Folder {
                    actions.push(Action::StartRename(row.key.clone()));
                } else if resp.double_clicked() && row.has_children {
                    actions.push(Action::Toggle(row.key.clone()));
                } else if resp.clicked() {
                    actions.push(Action::Select { key: row.key.clone(), shift, ctrl });
                }
            }

            if resp.drag_started() {
                let keys: Vec<String> = if is_selected && multi {
                    rows.iter().filter(|r| app.sb.selected.contains(&r.key)).map(|r| r.key.clone()).collect()
                } else {
                    vec![row.key.clone()]
                };
                let label = if keys.len() == 1 { row.name.clone() } else { format!("{} folders", keys.len()) };
                app.sb.drag = Some(TreeDrag { keys, label, target: None });
            }

            let key = row.key.clone();
            let kind = row.kind;
            crate::widgets::context_menu(&resp, |ui| {
                ui.set_min_width(170.0);
                if kind == NodeKind::Folder && ui.button("Rename").clicked() {
                    actions.push(Action::StartRename(key.clone()));
                    ui.close();
                }
                if ui.button("Reveal in Explorer").clicked() {
                    actions.push(Action::Reveal(key.clone()));
                    ui.close();
                }
                if kind == NodeKind::Folder {
                    ui.separator();
                    if ui.button("Remove from Disc").clicked() {
                        actions.push(Action::AskRemove(vec![key.clone()]));
                        ui.close();
                    }
                }
            });
        }
        // Room below the last row to drop things at the end of the list.
        ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    });
    app.sb.scroll_offset = out.state.offset().y;
    let viewport = out.inner_rect;

    // --- Drag in progress ------------------------------------------------------
    if app.sb.drag.is_some() {
        let cancel = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut target = None;
        if let Some(ptr) = pointer.filter(|p| viewport.contains(*p)) {
            let hit = row_rects.iter().find(|(_, r)| r.top() <= ptr.y && ptr.y < r.bottom());
            target = match hit {
                Some((i, r)) => {
                    let row = &rows[*i];
                    let f = (ptr.y - r.top()) / r.height();
                    Some(if f < 0.28 {
                        DropTarget::Before(row.key.clone())
                    } else if f > 0.72 {
                        DropTarget::After(row.key.clone())
                    } else {
                        DropTarget::Into(row.key.clone())
                    })
                }
                None if row_rects.last().is_some_and(|(_, r)| ptr.y >= r.bottom()) => Some(DropTarget::End),
                None => None,
            };
            let keys = app.sb.drag.as_ref().unwrap().keys.clone();
            if target.as_ref().is_some_and(|t| !target_valid(app, &keys, t)) {
                target = None;
            }

            // Auto-scroll near the edges.
            let edge = 28.0;
            let speed = if ptr.y < viewport.top() + edge {
                -(viewport.top() + edge - ptr.y) * 0.5
            } else if ptr.y > viewport.bottom() - edge {
                (ptr.y - (viewport.bottom() - edge)) * 0.5
            } else {
                0.0
            };
            if speed != 0.0 {
                app.sb.scroll_to = Some((app.sb.scroll_offset + speed).max(0.0));
                ctx.request_repaint();
            }

            // Hovering the middle of a collapsed folder opens it.
            match &target {
                Some(DropTarget::Into(k)) if !app.expanded.contains(k) && !app.model.children(k).is_empty() => {
                    match &app.sb.hover_expand {
                        Some((hk, at)) if hk == k => {
                            if at.elapsed() >= HOVER_EXPAND {
                                app.expanded.insert(k.clone());
                                app.mark_settings_dirty();
                                app.sb.hover_expand = None;
                            } else {
                                ctx.request_repaint_after(Duration::from_millis(60));
                            }
                        }
                        _ => {
                            app.sb.hover_expand = Some((k.clone(), Instant::now()));
                            ctx.request_repaint_after(Duration::from_millis(60));
                        }
                    }
                }
                _ => app.sb.hover_expand = None,
            }
        }

        // Indicator.
        let p = ui.painter().with_clip_rect(viewport);
        let find = |k: &str| rows.iter().position(|r| r.key == k).map(|i| (&rows[i], row_rects[i].1));
        let line = |y: f32, depth: usize| {
            let x = viewport.left() + 8.0 + depth as f32 * INDENT;
            p.line_segment([pos2(x, y), pos2(viewport.right() - 4.0, y)], Stroke::new(2.0, theme::ACCENT()));
            p.circle_stroke(pos2(x, y), 3.0, Stroke::new(1.5, theme::ACCENT()));
        };
        match &target {
            Some(DropTarget::Before(k)) => {
                if let Some((row, r)) = find(k) {
                    line(r.top(), row.depth);
                }
            }
            Some(DropTarget::After(k)) => {
                if let Some((row, r)) = find(k) {
                    let depth = if row.expanded { row.depth + 1 } else { row.depth };
                    line(r.bottom(), depth);
                }
            }
            Some(DropTarget::Into(k)) => {
                if let Some((_, r)) = find(k) {
                    p.rect_filled(r, 5.0, egui::Color32::from_white_alpha(18));
                    p.rect_stroke(r, 5.0, Stroke::new(1.5, theme::ACCENT()), StrokeKind::Inside);
                }
            }
            Some(DropTarget::End) => {
                if let Some((_, r)) = row_rects.last() {
                    line(r.bottom() + 1.0, 0);
                }
            }
            None => {}
        }

        // Ghost label following the pointer.
        if let Some(ptr) = pointer {
            let label = app.sb.drag.as_ref().unwrap().label.clone();
            egui::Area::new(Id::new("tree-drag-ghost"))
                .fixed_pos(ptr + vec2(14.0, 8.0))
                .order(egui::Order::Tooltip)
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(theme::BG_ACTIVE())
                        .stroke(Stroke::new(1.0, theme::BORDER()))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(label).font(theme::font_semibold(12.5)).color(theme::TEXT()));
                        });
                });
        }
        ctx.set_cursor_icon(if target.is_some() { egui::CursorIcon::Default } else { egui::CursorIcon::NotAllowed });

        if cancel {
            app.sb.drag = None;
            app.sb.hover_expand = None;
        } else if !primary_down {
            let drag = app.sb.drag.take().unwrap();
            app.sb.hover_expand = None;
            if let Some(t) = target {
                let (parent, before) = resolve(app, &t);
                app.move_nodes(&drag.keys, &parent, before.as_deref());
            }
        } else if let Some(d) = app.sb.drag.as_mut() {
            d.target = target;
        }
    }

    // --- Apply clicks ------------------------------------------------------------
    for a in actions {
        match a {
            Action::Toggle(k) => {
                if !app.expanded.remove(&k) {
                    app.expanded.insert(k);
                }
                app.mark_settings_dirty();
            }
            Action::Select { key, shift, ctrl } => {
                if shift {
                    let anchor = app.sb.anchor.clone().unwrap_or_else(|| app.active.clone());
                    let a = rows.iter().position(|r| r.key == anchor);
                    let b = rows.iter().position(|r| r.key == key);
                    if let (Some(a), Some(b)) = (a, b) {
                        let (lo, hi) = (a.min(b), a.max(b));
                        app.sb.selected = rows[lo..=hi].iter().map(|r| r.key.clone()).collect();
                    } else {
                        app.sb.selected = [key.clone()].into();
                    }
                } else if ctrl {
                    if app.sb.selected.is_empty() && !matches!(app.active.as_str(), "all" | "favorites" | "removed") {
                        app.sb.selected.insert(app.active.clone());
                    }
                    if !app.sb.selected.remove(&key) {
                        app.sb.selected.insert(key.clone());
                    }
                    app.sb.anchor = Some(key);
                } else {
                    app.sb.selected = [key.clone()].into();
                    app.sb.anchor = Some(key.clone());
                    app.set_active(&key);
                }
            }
            Action::StartRename(k) => {
                let name = app.settings.folders.iter().find(|f| f.id == k).map(|f| f.name.clone()).unwrap_or_default();
                app.sb.editing = Some((k, name));
                app.sb.editing_focus_requested = false;
            }
            Action::AskRemove(keys) => {
                let ids = removable(app, &keys);
                ask_remove(app, ids);
            }
            Action::Reveal(k) => {
                let (folder_id, sub) = crate::tree::parse_selection(&k);
                if let Some(f) = app.settings.folders.iter().find(|f| f.id == folder_id) {
                    let mut path = std::path::PathBuf::from(&f.folder_path);
                    for part in sub.split('/').filter(|s| !s.is_empty()) {
                        path.push(part);
                    }
                    crate::library::reveal_in_explorer(&path);
                }
            }
        }
    }

    // --- Keyboard (only while the sidebar has focus) -------------------------------
    if app.sb.focused && app.sb.editing.is_none() && app.confirm.is_none() && !ctx.egui_wants_keyboard_input() {
        let (del, sel_all, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Delete),
                i.modifiers.command && i.key_pressed(egui::Key::A),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if del {
            let mut keys: Vec<String> = app.sb.selected.iter().cloned().collect();
            if keys.is_empty() && !matches!(app.active.as_str(), "all" | "favorites" | "removed") {
                keys.push(app.active.clone());
            }
            let ids = removable(app, &keys);
            ask_remove(app, ids);
        }
        if sel_all {
            app.sb.selected = rows.iter().filter(|r| r.kind == NodeKind::Folder).map(|r| r.key.clone()).collect();
        }
        if esc && app.sb.drag.is_none() {
            app.sb.selected.clear();
        }
    }

    // --- Selection bar ---------------------------------------------------------------
    if app.sb.selected.len() > 1 {
        let keys: Vec<String> = app.sb.selected.iter().cloned().collect();
        let ids = removable(app, &keys);
        ui.add_space(6.0);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(bar, 8.0, theme::BG_PANEL_ALT());
        p.rect_stroke(bar, 8.0, Stroke::new(1.0, theme::BORDER()), StrokeKind::Inside);
        p.text(
            pos2(bar.left() + 12.0, bar.center().y),
            Align2::LEFT_CENTER,
            format!("{} selected", app.sb.selected.len()),
            theme::font(12.5),
            theme::TEXT2(),
        );
        let label = if ids.is_empty() { "Clear".to_string() } else { "Remove from Disc".to_string() };
        let font = theme::font_semibold(12.0);
        let w = p.layout_no_wrap(label.clone(), font.clone(), theme::TEXT()).size().x + 18.0;
        let br = Rect::from_min_max(pos2(bar.right() - 6.0 - w, bar.top() + 5.0), pos2(bar.right() - 6.0, bar.bottom() - 5.0));
        let resp = ui.interact(br, Id::new("sel-remove"), Sense::click());
        let p = ui.painter();
        p.rect_filled(br, 6.0, if resp.hovered() { theme::ACCENT_HOVER() } else { theme::ACCENT() });
        p.text(br.center(), Align2::CENTER_CENTER, label, font, theme::ACCENT_TEXT());
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            if ids.is_empty() {
                app.sb.selected.clear();
            } else {
                ask_remove(app, ids);
            }
        }
    }
}

/// Bottom of the column: the version, the theme picker, and an Update
/// button when a newer release is out.
fn footer(app: &mut DiscApp, ui: &mut egui::Ui) {
    use crate::update::{Status, VERSION};
    let frame = egui::Frame::new().fill(theme::BG_PANEL());
    egui::Panel::bottom("sidebar-footer").exact_size(30.0).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        let r = ui.max_rect();
        let p = ui.painter().clone();
        p.line_segment([pos2(r.left(), r.top()), pos2(r.right(), r.top())], Stroke::new(1.0, theme::GUIDE()));
        let ver = p.text(
            pos2(r.left() + 8.0, r.center().y + 2.0),
            Align2::LEFT_CENTER,
            format!("Disc Lite {VERSION}"),
            theme::font(11.5),
            theme::TEXT3(),
        );
        theme_picker(app, ui, pos2(ver.right() + 16.0, r.center().y + 2.0));

        let status = app.updater.status();
        // Checked automatically at launch: nothing to show unless there's an
        // update (or one is being installed).
        let (label, clickable, highlight) = match &status {
            Status::Available(rel) => (format!("Update to {}", rel.version), true, true),
            Status::Downloading(v) => (format!("Downloading {v}â€¦"), false, false),
            Status::Ready(_) => ("Restartingâ€¦".to_string(), false, false),
            Status::Idle | Status::Checking | Status::UpToDate | Status::Failed(_) => return,
        };
        let font = theme::font_semibold(11.5);
        let w = p.layout_no_wrap(label.clone(), font.clone(), theme::TEXT()).size().x + 16.0;
        let br = Rect::from_min_max(pos2(r.right() - 2.0 - w, r.top() + 5.0), pos2(r.right() - 2.0, r.bottom() - 1.0));
        let resp = ui.interact(br, Id::new("check-updates"), if clickable { Sense::click() } else { Sense::hover() });
        crate::widgets::hand(&resp, clickable);
        let p = ui.painter();
        if highlight {
            p.rect_filled(br, 6.0, if resp.hovered() { theme::ACCENT_HOVER() } else { theme::ACCENT() });
        } else if clickable && resp.hovered() {
            p.rect_filled(br, 6.0, theme::BG_HOVER());
        }
        let col = if highlight {
            theme::ACCENT_TEXT()
        } else if clickable && resp.hovered() {
            theme::TEXT()
        } else {
            theme::TEXT2()
        };
        p.text(br.center(), Align2::CENTER_CENTER, label, font, col);
        if resp.clicked() {
            match status {
                Status::Available(rel) => {
                    app.confirm = Some((
                        Confirm {
                            title: format!("Update to Disc Lite {}?", rel.version),
                            message: format!(
                                "You have {VERSION}. Disc Lite will download the new version, then close and reopen itself. Your folders and settings stay as they are."
                            ),
                            confirm_label: "Update".into(),
                        },
                        PendingConfirm::Update(rel),
                    ));
                }
                _ => {}
            }
        }
    });
}

/// The low-key palette icon, and the small theme menu it opens above it.
fn theme_picker(app: &mut DiscApp, ui: &mut egui::Ui, center: egui::Pos2) {
    let r = Rect::from_center_size(center, vec2(24.0, 24.0));
    let resp = ui.interact(r, Id::new("theme-icon"), Sense::click());
    crate::widgets::hand(&resp, true);
    let open = app.theme_menu_open;
    let p = ui.painter();
    if open || resp.hovered() {
        p.rect_filled(r, 6.0, theme::BG_HOVER());
    }
    let col = if open || resp.hovered() { theme::TEXT() } else { theme::TEXT3() };
    let hole = if open || resp.hovered() { theme::BG_HOVER() } else { theme::BG_PANEL() };
    icons::palette(p, r.center(), 15.0, col, hole);
    let resp = resp.on_hover_text("Theme");
    if resp.clicked() {
        app.theme_menu_open = !app.theme_menu_open;
    }
    if !app.theme_menu_open {
        app.theme_menu_rect = None;
        return;
    }

    let ctx = ui.ctx().clone();
    let mut chosen: Option<&'static str> = None;
    let area = egui::Area::new(Id::new("theme-menu"))
        .order(egui::Order::Foreground)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(pos2(r.left() - 4.0, r.top() - 6.0))
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(theme::BG_PANEL_ALT())
                .stroke(Stroke::new(1.0, theme::BORDER()))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::same(6))
                .shadow(egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: egui::Color32::from_black_alpha(140) })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                    let (head, _) = ui.allocate_exact_size(vec2(180.0, 22.0), Sense::hover());
                    ui.painter().text(
                        pos2(head.left() + 8.0, head.center().y),
                        Align2::LEFT_CENTER,
                        "THEME",
                        theme::font_semibold(10.5),
                        theme::TEXT3(),
                    );
                    for t in &theme::THEMES {
                        let (row, _) = ui.allocate_exact_size(vec2(180.0, 32.0), Sense::hover());
                        let rr = ui.interact(row, Id::new(("theme-row", t.id)), Sense::click());
                        crate::widgets::hand(&rr, true);
                        let current = theme::current().id == t.id;
                        let p = ui.painter();
                        if rr.hovered() {
                            p.rect_filled(row, 6.0, theme::BG_HOVER());
                        }
                        // Swatch: background, panel and accent of that theme.
                        let sx = row.left() + 14.0;
                        let cy = row.center().y;
                        for (i, c) in [t.bg_app, t.bg_active, t.accent].into_iter().enumerate() {
                            let cc = pos2(sx + i as f32 * 9.0, cy);
                            p.circle_filled(cc, 7.0, c);
                            p.circle_stroke(cc, 7.0, Stroke::new(1.0, t.border));
                        }
                        p.text(
                            pos2(row.left() + 46.0, cy),
                            Align2::LEFT_CENTER,
                            t.label,
                            if current { theme::font_semibold(13.0) } else { theme::font(13.0) },
                            if current { theme::TEXT() } else { theme::TEXT2() },
                        );
                        if current {
                            // Check mark.
                            let c = pos2(row.right() - 16.0, cy);
                            let st = Stroke::new(1.6, theme::TEXT());
                            p.line_segment([c + vec2(-4.5, 0.0), c + vec2(-1.5, 3.0)], st);
                            p.line_segment([c + vec2(-1.5, 3.0), c + vec2(4.5, -3.5)], st);
                        }
                        if rr.clicked() {
                            chosen = Some(t.id);
                        }
                    }
                });
        });
    app.theme_menu_rect = Some(area.response.rect);
    if let Some(id) = chosen {
        app.set_theme(&ctx, id);
    }
    // Close on Escape, or a click anywhere outside the menu and the icon.
    let clicked_outside = ctx.input(|i| i.pointer.any_click())
        && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pos| !area.response.rect.contains(pos) && !r.contains(pos));
    if clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.theme_menu_open = false;
    }
}
