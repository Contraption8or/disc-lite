//! Shared bits of UI: one-line text that ends in "…" when it doesn't fit,
//! the toolbar buttons, and the confirmation dialog.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Response, Sense,
    StrokeKind, Ui, Vec2, pos2, text::LayoutJob,
};

use crate::theme;

pub fn elided(painter: &Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    painter.layout_job(job)
}

/// Draws `galley` vertically centered on `y`, starting at `x`.
pub fn draw_left(painter: &Painter, x: f32, y: f32, galley: Arc<Galley>) {
    let pos = pos2(x, y - galley.size().y / 2.0);
    painter.galley(pos, galley, theme::TEXT);
}

#[derive(Clone, Copy, PartialEq)]
pub enum ButtonKind {
    /// White fill, black text — the one primary action.
    Primary,
    /// Text only, highlight on hover.
    Ghost,
}

/// A toolbar button. `icon` paints into a square at the left, if given.
pub fn button(
    ui: &mut Ui,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
    icon: Option<&dyn Fn(&Painter, Pos2, Color32)>,
) -> Response {
    let font = theme::font_semibold(12.5);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, theme::TEXT);
    let icon_w = if icon.is_some() { 18.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + icon_w + 24.0, 30.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    let hovered = enabled && resp.hovered();
    let (bg, fg) = match kind {
        ButtonKind::Primary => (if hovered { theme::ACCENT_HOVER } else { theme::ACCENT }, theme::ACCENT_TEXT),
        ButtonKind::Ghost => (if hovered { theme::BG_HOVER } else { Color32::TRANSPARENT }, theme::TEXT),
    };
    let alpha = if enabled { 1.0 } else { 0.4 };
    p.rect_filled(rect, 6.0, bg.gamma_multiply(alpha));
    let fg = fg.gamma_multiply(alpha);
    let mut x = rect.left() + 12.0;
    if let Some(icon) = icon {
        icon(p, pos2(x + 6.0, rect.center().y), fg);
        x += icon_w;
    }
    p.galley_with_override_text_color(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub struct Confirm {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
}

pub enum ConfirmResult {
    Open,
    Confirmed,
    Cancelled,
}

pub fn confirm_modal(ctx: &egui::Context, c: &Confirm) -> ConfirmResult {
    let mut result = ConfirmResult::Open;
    let frame = egui::Frame::new()
        .fill(theme::BG_PANEL_ALT)
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(20));
    let resp = egui::Modal::new(egui::Id::new("confirm-modal"))
        .frame(frame)
        .backdrop_color(Color32::from_black_alpha(150))
        .show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new(&c.title).font(theme::font_semibold(16.0)).color(theme::TEXT));
            ui.add_space(8.0);
            ui.label(egui::RichText::new(&c.message).font(theme::font(13.0)).color(theme::TEXT2));
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, &c.confirm_label, ButtonKind::Primary, true, None).clicked() {
                    result = ConfirmResult::Confirmed;
                }
                if button(ui, "Cancel", ButtonKind::Ghost, true, None).clicked() {
                    result = ConfirmResult::Cancelled;
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                result = ConfirmResult::Confirmed;
            }
        });
    if resp.should_close() && matches!(result, ConfirmResult::Open) {
        result = ConfirmResult::Cancelled;
    }
    result
}

/// A centered "nothing here" message with an optional action button.
pub fn empty_state(
    ui: &mut Ui,
    icon: impl FnOnce(&Painter, Pos2, Color32),
    title: &str,
    text: &str,
    action: Option<&str>,
) -> bool {
    let mut clicked = false;
    let avail = ui.available_rect_before_wrap();
    let center = avail.center();
    let p = ui.painter();
    let icon_rect = Rect::from_center_size(pos2(center.x, center.y - 60.0), Vec2::splat(56.0));
    p.rect_filled(icon_rect, 14.0, theme::BG_PANEL_ALT);
    p.rect_stroke(icon_rect, 14.0, egui::Stroke::new(1.0, theme::BORDER), StrokeKind::Inside);
    icon(p, icon_rect.center(), theme::TEXT);
    p.text(pos2(center.x, center.y - 14.0), Align2::CENTER_CENTER, title, theme::font_semibold(15.0), theme::TEXT);
    let mut job = LayoutJob::simple(text.to_string(), theme::font(13.0), theme::TEXT2, 340.0);
    job.halign = egui::Align::Center;
    let galley = p.layout_job(job);
    let text_h = galley.size().y;
    p.galley(pos2(center.x, center.y + 2.0), galley, theme::TEXT2);
    if let Some(label) = action {
        let font = theme::font_semibold(13.0);
        let w = p.layout_no_wrap(label.to_string(), font.clone(), theme::ACCENT_TEXT).size().x + 28.0;
        let r = Rect::from_center_size(pos2(center.x, center.y + text_h + 30.0), egui::vec2(w, 32.0));
        let resp = ui.interact(r, ui.id().with("empty-action"), Sense::click());
        ui.painter().rect_filled(r, 7.0, if resp.hovered() { theme::ACCENT_HOVER } else { theme::ACCENT });
        ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, theme::ACCENT_TEXT);
        clicked = resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
    }
    clicked
}

/// Shows the pointing-hand cursor while `resp` is hovered (and `enabled`).
/// One place for it, so every clickable icon gets the same cursor.
pub fn hand(resp: &Response, enabled: bool) {
    if enabled && resp.hovered() {
        resp.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}
