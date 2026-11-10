//! Color themes, and Segoe UI (Windows' own UI font) with egui's built-in
//! fonts kept as fallbacks.
//!
//! Colors are looked up through the functions below (`theme::TEXT()` etc.)
//! rather than constants, so switching themes takes effect on the very next
//! frame without restarting.

#![allow(non_snake_case)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};

pub struct Palette {
    /// Saved in settings.
    pub id: &'static str,
    pub label: &'static str,
    pub bg_app: Color32,
    pub bg_panel: Color32,
    pub bg_panel_alt: Color32,
    pub bg_hover: Color32,
    pub bg_active: Color32,
    pub border: Color32,
    pub guide: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_text: Color32,
    pub waveform: Color32,
    pub waveform_hover: Color32,
    pub waveform_preview: Color32,
    pub waveform_base: Color32,
    pub wave_played: Color32,
    pub row_hover: Color32,
    pub row_current: Color32,
    pub danger: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const THEMES: [Palette; 2] = [
    Palette {
        id: "mono",
        label: "Mono",
        bg_app: rgb(0x000000),
        bg_panel: rgb(0x070707),
        bg_panel_alt: rgb(0x101010),
        bg_hover: rgb(0x1a1a1a),
        bg_active: rgb(0x262626),
        border: rgb(0x2a2a2a),
        guide: rgb(0x1e1e1e),
        text: rgb(0xffffff),
        text2: rgb(0xa8a8a8),
        text3: rgb(0x787878),
        accent: rgb(0xffffff),
        accent_hover: rgb(0xd6d6d6),
        accent_text: rgb(0x000000),
        waveform: rgb(0x383838),
        waveform_hover: rgb(0x4a4a4a),
        waveform_preview: rgb(0x808080),
        waveform_base: rgb(0x222222),
        wave_played: rgb(0xffffff),
        row_hover: rgb(0x0d0d0d),
        row_current: rgb(0x141414),
        danger: rgb(0xef5b5b),
    },
    // Dusky wine with a peach accent — a darker take on a warm analytics
    // dashboard look, so it still sits comfortably as a dark theme.
    Palette {
        id: "plum",
        label: "Plum",
        bg_app: rgb(0x1e1317),
        bg_panel: rgb(0x26181e),
        bg_panel_alt: rgb(0x321f27),
        bg_hover: rgb(0x3a252e),
        bg_active: rgb(0x482e39),
        border: rgb(0x553844),
        guide: rgb(0x3a2630),
        text: rgb(0xf7e8e2),
        text2: rgb(0xd0b5ae),
        text3: rgb(0x9c7f82),
        accent: rgb(0xf2cdb9),
        accent_hover: rgb(0xe6b9a2),
        accent_text: rgb(0x2a1a20),
        waveform: rgb(0x573a45),
        waveform_hover: rgb(0x6a4753),
        waveform_preview: rgb(0xa98a88),
        waveform_base: rgb(0x3d2831),
        wave_played: rgb(0xf2cdb9),
        row_hover: rgb(0x26181e),
        row_current: rgb(0x33212a),
        danger: rgb(0xf07f7f),
    },
];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn current() -> &'static Palette {
    &THEMES[CURRENT.load(Ordering::Relaxed).min(THEMES.len() - 1)]
}

pub fn BG_APP() -> Color32 { current().bg_app }
pub fn BG_PANEL() -> Color32 { current().bg_panel }
pub fn BG_PANEL_ALT() -> Color32 { current().bg_panel_alt }
pub fn BG_HOVER() -> Color32 { current().bg_hover }
pub fn BG_ACTIVE() -> Color32 { current().bg_active }
pub fn BORDER() -> Color32 { current().border }
pub fn GUIDE() -> Color32 { current().guide }
pub fn TEXT() -> Color32 { current().text }
pub fn TEXT2() -> Color32 { current().text2 }
pub fn TEXT3() -> Color32 { current().text3 }
pub fn ACCENT() -> Color32 { current().accent }
pub fn ACCENT_HOVER() -> Color32 { current().accent_hover }
pub fn ACCENT_TEXT() -> Color32 { current().accent_text }
pub fn WAVEFORM() -> Color32 { current().waveform }
pub fn WAVEFORM_HOVER() -> Color32 { current().waveform_hover }
pub fn WAVEFORM_PREVIEW() -> Color32 { current().waveform_preview }
pub fn WAVEFORM_BASE() -> Color32 { current().waveform_base }
pub fn WAVE_PLAYED() -> Color32 { current().wave_played }
pub fn ROW_HOVER() -> Color32 { current().row_hover }
pub fn ROW_CURRENT() -> Color32 { current().row_current }
pub fn DANGER() -> Color32 { current().danger }

pub const SEMIBOLD: &str = "semibold";

pub fn font(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn font_semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

fn load_system_font(name: &str) -> Option<Vec<u8>> {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from)?.join("Fonts");
    std::fs::read(dir.join(name)).ok()
}

/// Fonts + the saved theme. Called once at startup.
pub fn install(ctx: &egui::Context, theme_id: &str) {
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    if let Some(bytes) = load_system_font("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "segoe".into());
    }
    let mut semibold = fallbacks.clone();
    if let Some(bytes) = load_system_font("seguisb.ttf") {
        fonts.font_data.insert("segoe-sb".into(), Arc::new(FontData::from_owned(bytes)));
        semibold.insert(0, "segoe-sb".into());
    } else if fonts.font_data.contains_key("segoe") {
        semibold.insert(0, "segoe".into());
    }
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);

    set_theme(ctx, theme_id);
}

/// Switches the palette and restyles egui's own widgets (menus, text
/// boxes, tooltips, scrollbars) to match. Unknown ids fall back to Mono.
pub fn set_theme(ctx: &egui::Context, theme_id: &str) {
    let idx = THEMES.iter().position(|t| t.id == theme_id).unwrap_or(0);
    CURRENT.store(idx, Ordering::Relaxed);
    let t = current();
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        v.dark_mode = true;
        // The lighter panel color sits behind everything, so the title bar,
        // folder column and player read as one frame around the track list.
        v.panel_fill = t.bg_panel;
        v.window_fill = t.bg_panel_alt;
        v.extreme_bg_color = t.bg_panel_alt;
        v.faint_bg_color = t.bg_panel;
        v.override_text_color = Some(t.text);
        v.window_stroke = egui::Stroke::new(1.0, t.border);
        v.selection.bg_fill = t.accent.gamma_multiply(0.3);
        v.selection.stroke = egui::Stroke::new(1.0, t.text);
        v.text_cursor.stroke = egui::Stroke::new(1.5, t.text);
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, t.border);
        v.widgets.inactive.bg_fill = t.bg_panel_alt;
        v.widgets.inactive.weak_bg_fill = t.bg_panel_alt;
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, t.border);
        v.widgets.hovered.bg_fill = t.bg_hover;
        v.widgets.hovered.weak_bg_fill = t.bg_hover;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, t.text3);
        v.widgets.active.bg_fill = t.bg_active;
        v.widgets.active.weak_bg_fill = t.bg_active;
        v.widgets.active.bg_stroke = egui::Stroke::new(1.0, t.text2);
        v.popup_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(160) };
        // Thin scrollbars that float over content and only show up when needed.
        style.spacing.scroll = egui::style::ScrollStyle::thin();
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.floating_allocated_width = 0.0;
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.interaction.tooltip_delay = 0.6;
    });
    ctx.request_repaint();
}
