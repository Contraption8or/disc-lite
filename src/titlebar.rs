//! Our own title bar, in the theme's panel color so it wraps around the
//! track list together with the folder column (like Discord's). The window
//! is undecorated, but moving still uses Windows' own move loop (so dragging
//! to a screen edge snaps), double-click maximizes, and the edges resize.

use eframe::egui::{self, Color32, CursorIcon, Id, Rect, ResizeDirection, Sense, Stroke, ViewportCommand, pos2, vec2};

use crate::theme::{self, Palette};

pub const HEIGHT: f32 = 32.0;
const BUTTON_W: f32 = 46.0;
/// How close to the window edge the pointer must be to resize.
const EDGE: f32 = 6.0;
/// Windows' own close-button red.
const CLOSE_RED: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);
const CLOSE_RED_PRESSED: Color32 = Color32::from_rgb(0x9b, 0x22, 0x16);

pub fn show(ui: &mut egui::Ui, ctx: &egui::Context) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let frame = egui::Frame::new().fill(theme::BG_PANEL());
    egui::Panel::top("titlebar").exact_size(HEIGHT).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        let bar = ui.max_rect();
        let buttons_w = BUTTON_W * 3.0;
        let drag_rect = Rect::from_min_max(bar.min, pos2(bar.right() - buttons_w, bar.bottom()));

        // The empty part of the bar moves the window.
        let drag = ui.interact(drag_rect, Id::new("titlebar-drag"), Sense::click_and_drag());
        if drag.double_clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        } else if drag.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }

        let mut x = bar.right() - buttons_w;
        let mut button = |kind: Button| {
            let r = Rect::from_min_size(pos2(x, bar.top()), vec2(BUTTON_W, HEIGHT));
            x += BUTTON_W;
            window_button(ui, ctx, r, kind, maximized)
        };
        if button(Button::Minimize).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        if button(Button::Maximize).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        if button(Button::Close).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    });
    if !maximized {
        resize_edges(ctx);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

fn window_button(ui: &mut egui::Ui, ctx: &egui::Context, r: Rect, kind: Button, maximized: bool) -> egui::Response {
    let id = Id::new(("window-button", kind as u8));
    let resp = ui.interact(r, id, Sense::click());
    let pressed = resp.is_pointer_button_down_on();
    // Short fades in and out, instead of the hover snapping on and off.
    let t = ctx.animate_bool_with_time(id.with("hover"), resp.hovered() || pressed, 0.12);
    let p = ui.painter();
    let (bg, fg) = if kind == Button::Close {
        let full = if pressed { CLOSE_RED_PRESSED } else { CLOSE_RED };
        (fade(full, t), lerp(theme::TEXT2(), Color32::WHITE, t))
    } else {
        let full = if pressed { theme::BG_ACTIVE() } else { theme::BG_HOVER() };
        (fade(full, t), lerp(theme::TEXT2(), theme::TEXT(), t))
    };
    if t > 0.0 {
        p.rect_filled(r, 0.0, bg);
    }
    // Icons drawn on whole pixels so the 1px lines stay crisp.
    let ppp = ctx.pixels_per_point();
    let snap = |v: f32| (v * ppp).round() / ppp + 0.5 / ppp;
    let c = pos2(snap(r.center().x), snap(r.center().y));
    let s = 5.0;
    let st = Stroke::new(1.0, fg);
    match kind {
        Button::Minimize => {
            p.line_segment([pos2(c.x - s, c.y), pos2(c.x + s, c.y)], st);
        }
        Button::Maximize if maximized => {
            // Restore: a square with another peeking out behind it.
            let front = Rect::from_min_max(pos2(c.x - s, c.y - s + 2.0), pos2(c.x + s - 2.0, c.y + s));
            p.rect_stroke(front, 1.0, st, egui::StrokeKind::Middle);
            p.line_segment([pos2(c.x - s + 2.0, c.y - s + 2.0), pos2(c.x - s + 2.0, c.y - s)], st);
            p.line_segment([pos2(c.x - s + 2.0, c.y - s), pos2(c.x + s, c.y - s)], st);
            p.line_segment([pos2(c.x + s, c.y - s), pos2(c.x + s, c.y + s - 2.0)], st);
            p.line_segment([pos2(c.x + s, c.y + s - 2.0), pos2(c.x + s - 2.0, c.y + s - 2.0)], st);
        }
        Button::Maximize => {
            let sq = Rect::from_min_max(pos2(c.x - s, c.y - s), pos2(c.x + s, c.y + s));
            p.rect_stroke(sq, 1.0, st, egui::StrokeKind::Middle);
        }
        Button::Close => {
            p.line_segment([pos2(c.x - s, c.y - s), pos2(c.x + s, c.y + s)], st);
            p.line_segment([pos2(c.x - s, c.y + s), pos2(c.x + s, c.y - s)], st);
        }
    }
    let tip = match kind {
        Button::Minimize => "Minimize",
        Button::Maximize if maximized => "Restore",
        Button::Maximize => "Maximize",
        Button::Close => "Close",
    };
    resp.on_hover_text(tip)
}

fn fade(c: Color32, t: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (t.clamp(0.0, 1.0) * 255.0) as u8)
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Without a native frame, the window's edges are ours to handle: show the
/// resize cursor near them and hand the drag to Windows on press.
fn resize_edges(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else { return };
    let (l, r) = (pos.x - screen.left() < EDGE, screen.right() - pos.x < EDGE);
    let (t, b) = (pos.y - screen.top() < EDGE, screen.bottom() - pos.y < EDGE);
    let dir = match (l, r, t, b) {
        (true, _, true, _) => Some((ResizeDirection::NorthWest, CursorIcon::ResizeNwSe)),
        (_, true, true, _) => Some((ResizeDirection::NorthEast, CursorIcon::ResizeNeSw)),
        (true, _, _, true) => Some((ResizeDirection::SouthWest, CursorIcon::ResizeNeSw)),
        (_, true, _, true) => Some((ResizeDirection::SouthEast, CursorIcon::ResizeNwSe)),
        (true, ..) => Some((ResizeDirection::West, CursorIcon::ResizeHorizontal)),
        (_, true, ..) => Some((ResizeDirection::East, CursorIcon::ResizeHorizontal)),
        (_, _, true, _) => Some((ResizeDirection::North, CursorIcon::ResizeVertical)),
        (_, _, _, true) => Some((ResizeDirection::South, CursorIcon::ResizeVertical)),
        _ => None,
    };
    let Some((dir, cursor)) = dir else { return };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
}

/// Windows 11 touches: rounded corners and a subtle border that blends into
/// the title bar.
/// Returns false if the window handle wasn't ready yet (try next frame).
pub fn apply(frame: &eframe::Frame, p: &Palette) -> bool {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use std::ffi::c_void;
        const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
        const DWMWA_BORDER_COLOR: u32 = 34;
        const DWMWA_CAPTION_COLOR: u32 = 35;
        const DWMWCP_ROUND: u32 = 2;
        #[link(name = "dwmapi")]
        unsafe extern "system" {
            fn DwmSetWindowAttribute(hwnd: *mut c_void, attr: u32, value: *const c_void, size: u32) -> i32;
        }
        let Ok(handle) = frame.window_handle() else { return false };
        let RawWindowHandle::Win32(w) = handle.as_raw() else { return false };
        let hwnd = w.hwnd.get() as *mut c_void;
        // Barely lighter than the title bar, so the top edge melts into the bar
        // (like Discord) instead of drawing a bright line around the window.
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * 0.3).round() as u8;
        let (bar, line) = (p.bg_panel, p.border);
        let c = eframe::egui::Color32::from_rgb(mix(bar.r(), line.r()), mix(bar.g(), line.g()), mix(bar.b(), line.b()));
        // COLORREF is 0x00BBGGRR.
        let border = (c.r() as u32) | ((c.g() as u32) << 8) | ((c.b() as u32) << 16);
        // SAFETY: hwnd is our own live window; both values are 4-byte DWORDs.
        unsafe {
            DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &DWMWCP_ROUND as *const u32 as *const _, 4);
            DwmSetWindowAttribute(hwnd, DWMWA_BORDER_COLOR, &border as *const u32 as *const _, 4);
            // With the native frame hidden (but its shadow kept), Windows still
            // leaves a 1px strip of its title bar along the very top; paint it
            // in our bar color so it disappears.
            let bar = (p.bg_panel.r() as u32) | ((p.bg_panel.g() as u32) << 8) | ((p.bg_panel.b() as u32) << 16);
            DwmSetWindowAttribute(hwnd, DWMWA_CAPTION_COLOR, &bar as *const u32 as *const _, 4);
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = (frame, p);
        true
    }
}

