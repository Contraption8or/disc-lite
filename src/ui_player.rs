//! The bar along the bottom: transport, what's playing, shuffle, volume, pin.

use eframe::egui::{self, Align2, Rect, Sense, pos2, vec2};

use crate::app::DiscApp;
use crate::model::format_duration;
use crate::{icons, theme, widgets};

const HEIGHT: f32 = 64.0;

pub fn show(app: &mut DiscApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    let frame = egui::Frame::new().fill(theme::BG_PANEL());
    egui::Panel::bottom("player").exact_size(HEIGHT).resizable(false).frame(frame).show(ui, |ui| {
        let full = ui.max_rect();
        let p = ui.painter().clone();
        p.line_segment([full.left_top(), full.right_top()], egui::Stroke::new(1.0, theme::BORDER()));

        let duration = app.current_duration();
        let pos = app.position();
        let has_track = app.current.is_some() && app.audio.as_ref().is_some_and(|a| a.is_active());

        // Progress line across the very top — click/drag to seek.
        let bar = Rect::from_min_size(full.left_top(), vec2(full.width(), 6.0));
        let bar_resp = ui.interact(bar, ui.id().with("progress"), Sense::click_and_drag());
        let frac = if duration > 0.0 && has_track { (pos / duration).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let thick = if bar_resp.hovered() || bar_resp.dragged() { 4.0 } else { 2.0 };
        let line = Rect::from_min_size(full.left_top(), vec2(full.width(), thick));
        p.rect_filled(line, 0.0, theme::BG_ACTIVE());
        p.rect_filled(Rect::from_min_size(line.min, vec2(line.width() * frac, thick)), 0.0, theme::ACCENT());
        if has_track && (bar_resp.clicked() || bar_resp.dragged()) {
            if let Some(ptr) = bar_resp.interact_pointer_pos() {
                let f = ((ptr.x - full.left()) / full.width()).clamp(0.0, 1.0) as f64;
                if let Some(t) = app.current.clone() {
                    app.seek_to(&t, f);
                }
            }
        }
        if has_track {
            bar_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        }

        let cy = full.center().y + 2.0;
        let mut x = full.left() + 14.0;

        // Transport.
        let can_prev = app.queue_index.is_some_and(|i| i > 0);
        let can_next = app.queue_index.is_some();
        let playing = app.is_playing();
        let r = Rect::from_center_size(pos2(x + 14.0, cy), vec2(28.0, 28.0));
        if round_button(ui, r, "prev", can_prev, |p, c, col| icons::skip(p, c, 14.0, col, -1.0)).clicked() {
            app.play_prev();
        }
        x += 32.0;
        let r = Rect::from_center_size(pos2(x + 17.0, cy), vec2(34.0, 34.0));
        let play_resp = ui.interact(r, ui.id().with("play"), Sense::click());
        crate::widgets::hand(&play_resp, app.current.is_some());
        let enabled = app.current.is_some();
        p.circle_filled(
            r.center(),
            17.0,
            if !enabled {
                theme::BG_ACTIVE()
            } else if play_resp.hovered() {
                theme::ACCENT_HOVER()
            } else {
                theme::ACCENT()
            },
        );
        let icon_col = if enabled { theme::ACCENT_TEXT() } else { theme::TEXT3() };
        if playing {
            icons::pause(&p, r.center(), 14.0, icon_col);
        } else {
            icons::play(&p, r.center() + vec2(1.0, 0.0), 14.0, icon_col);
        }
        if enabled && play_resp.clicked() {
            if let Some(t) = app.current.clone() {
                app.toggle_play(&t, false);
            }
        }
        x += 38.0;
        let r = Rect::from_center_size(pos2(x + 14.0, cy), vec2(28.0, 28.0));
        if round_button(ui, r, "next", can_next, |p, c, col| icons::skip(p, c, 14.0, col, 1.0)).clicked() {
            app.play_next();
        }
        x += 44.0;

        // Right side, laid out from the right edge inward.
        let mut rx = full.right() - 14.0;
        let pin_r = Rect::from_center_size(pos2(rx - 14.0, cy), vec2(28.0, 28.0));
        let pinned = app.pinned;
        let pin = round_button(ui, pin_r, "pin", true, |p, c, col| {
            icons::lock(p, c, 15.0, if pinned { theme::TEXT() } else { col }, pinned)
        })
        .on_hover_text(if pinned { "Unpin (stop staying on top)" } else { "Pin on top of other windows" });
        if pinned {
            p.circle_filled(pos2(pin_r.center().x, pin_r.bottom() + 1.0), 1.5, theme::TEXT());
        }
        if pin.clicked() {
            app.toggle_pin(ctx);
        }
        rx -= 40.0;

        let slider_w = 96.0;
        let slider = Rect::from_min_max(pos2(rx - slider_w, cy - 8.0), pos2(rx, cy + 8.0));
        let sresp = ui.interact(slider, ui.id().with("volume"), Sense::click_and_drag());
        crate::widgets::hand(&sresp, true);
        if sresp.clicked() || sresp.dragged() {
            if let Some(ptr) = sresp.interact_pointer_pos() {
                app.set_volume(((ptr.x - slider.left()) / slider.width()).clamp(0.0, 1.0));
            }
        }
        if sresp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                app.set_volume(app.settings.volume + scroll / 1000.0);
            }
        }
        let vol = app.settings.volume;
        let track = Rect::from_center_size(slider.center(), vec2(slider.width(), 3.0));
        p.rect_filled(track, 1.5, theme::BG_ACTIVE());
        let active = sresp.hovered() || sresp.dragged();
        p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * vol, 3.0)), 1.5, if active { theme::TEXT() } else { theme::TEXT2() });
        let knob_x = track.left() + track.width() * vol;
        p.circle_filled(pos2(knob_x, track.center().y), if active { 6.0 } else { 4.5 }, theme::TEXT());
        sresp.on_hover_text(format!("Volume {}%", (vol * 100.0).round()));
        rx -= slider_w + 6.0;

        let spk_r = Rect::from_center_size(pos2(rx - 13.0, cy), vec2(26.0, 26.0));
        if round_button(ui, spk_r, "mute", true, |p, c, col| icons::speaker(p, c, 16.0, col, vol)).clicked() {
            app.toggle_mute();
        }
        rx -= 34.0;

        let shuffle_on = app.settings.shuffle;
        let sh_r = Rect::from_center_size(pos2(rx - 15.0, cy), vec2(30.0, 30.0));
        let tip = if shuffle_on { "Shuffle: ON — click to turn off (S)" } else { "Shuffle: OFF — click to turn on (S)" };
        if toggle_button(ui, sh_r, "shuffle", shuffle_on, |p, c, col| icons::shuffle(p, c, 16.0, col))
            .on_hover_text(tip)
            .clicked()
        {
            app.toggle_shuffle();
        }
        rx -= 36.0;

        let loop_on = app.settings.loop_one;
        let lp_r = Rect::from_center_size(pos2(rx - 15.0, cy), vec2(30.0, 30.0));
        let tip = if loop_on { "Loop: ON — repeating this track (L)" } else { "Loop: OFF — click to repeat this track (L)" };
        if toggle_button(ui, lp_r, "loop", loop_on, |p, c, col| icons::repeat_one(p, c, 16.0, col))
            .on_hover_text(tip)
            .clicked()
        {
            app.toggle_loop();
        }
        rx -= 42.0;

        // Middle: what's playing.
        let text_w = (rx - x - 12.0).max(0.0);
        match &app.current {
            Some(t) => {
                let title = widgets::elided(&p, &t.stem, theme::font_semibold(13.5), theme::TEXT(), text_w);
                widgets::draw_left(&p, x, cy - 8.0, title);
                let mut meta = if duration > 0.0 {
                    format!("{} / {}", format_duration(pos.min(duration)), format_duration(duration))
                } else {
                    format_duration(pos)
                };
                if let Some(i) = app.queue_index {
                    meta.push_str(&format!("   ·   {} of {}", i + 1, app.queue.len()));
                }
                let meta = widgets::elided(&p, &meta, theme::font(12.0), theme::TEXT3(), text_w);
                p.galley(pos2(x, cy + 9.0 - meta.size().y / 2.0), meta, theme::TEXT3());
            }
            None => {
                p.text(pos2(x, cy), Align2::LEFT_CENTER, "Nothing playing", theme::font(13.0), theme::TEXT3());
            }
        }
    });
}

fn round_button(
    ui: &mut egui::Ui,
    r: Rect,
    id: &str,
    enabled: bool,
    draw: impl FnOnce(&egui::Painter, egui::Pos2, egui::Color32),
) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(r, ui.id().with(id), sense);
    crate::widgets::hand(&resp, enabled);
    let p = ui.painter();
    if enabled && resp.hovered() {
        p.circle_filled(r.center(), r.width() / 2.0, theme::BG_HOVER());
    }
    let col = if !enabled {
        theme::TEXT3().gamma_multiply(0.6)
    } else if resp.hovered() {
        theme::TEXT()
    } else {
        theme::TEXT2()
    };
    draw(p, r.center(), col);
    resp
}

/// An on/off button. On: filled highlight, bright icon and a dot under it.
/// Off: no fill and a dimmed icon, so the two states can't be confused.
fn toggle_button(
    ui: &mut egui::Ui,
    r: Rect,
    id: &str,
    on: bool,
    draw: impl FnOnce(&egui::Painter, egui::Pos2, egui::Color32),
) -> egui::Response {
    let resp = ui.interact(r, ui.id().with(id), Sense::click());
    crate::widgets::hand(&resp, true);
    let p = ui.painter();
    let hovered = resp.hovered();
    if on {
        p.rect_filled(r, 8.0, if hovered { theme::BORDER() } else { theme::BG_ACTIVE() });
        draw(p, r.center(), theme::TEXT());
        p.circle_filled(pos2(r.center().x, r.bottom() + 3.0), 2.0, theme::TEXT());
    } else {
        if hovered {
            p.rect_filled(r, 8.0, theme::BG_HOVER());
        }
        draw(p, r.center(), if hovered { theme::TEXT2() } else { theme::TEXT3().gamma_multiply(0.8) });
    }
    resp
}
