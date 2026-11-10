//! The one fixed black-and-white palette, and Segoe UI (Windows' own UI
//! font) with egui's built-in fonts kept as fallbacks.

use std::sync::Arc;

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};

pub const BG_APP: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x07, 0x07, 0x07);
pub const BG_PANEL_ALT: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);
pub const BG_HOVER: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
pub const BG_ACTIVE: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub const BORDER: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub const TEXT: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const TEXT2: Color32 = Color32::from_rgb(0xa8, 0xa8, 0xa8);
pub const TEXT3: Color32 = Color32::from_rgb(0x78, 0x78, 0x78);
pub const ACCENT: Color32 = Color32::WHITE;
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xd6, 0xd6, 0xd6);
pub const ACCENT_TEXT: Color32 = Color32::BLACK;
pub const WAVEFORM: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
pub const WAVEFORM_HOVER: Color32 = Color32::from_rgb(0x4a, 0x4a, 0x4a);
pub const WAVEFORM_PREVIEW: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);
pub const WAVEFORM_BASE: Color32 = Color32::from_rgb(0x22, 0x22, 0x22);
pub const ROW_HOVER: Color32 = Color32::from_rgb(0x0d, 0x0d, 0x0d);
pub const ROW_CURRENT: Color32 = Color32::from_rgb(0x14, 0x14, 0x14);
pub const GUIDE: Color32 = Color32::from_rgb(0x1e, 0x1e, 0x1e);
pub const DANGER: Color32 = Color32::from_rgb(0xef, 0x5b, 0x5b);

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

pub fn install(ctx: &egui::Context) {
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

    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = BG_APP;
        v.window_fill = BG_PANEL_ALT;
        v.extreme_bg_color = BG_PANEL_ALT;
        v.faint_bg_color = BG_PANEL;
        v.override_text_color = Some(TEXT);
        v.window_stroke = egui::Stroke::new(1.0, BORDER);
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(255, 255, 255, 70);
        v.selection.stroke = egui::Stroke::new(1.0, TEXT);
        v.text_cursor.stroke = egui::Stroke::new(1.5, TEXT);
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        v.widgets.inactive.bg_fill = BG_PANEL_ALT;
        v.widgets.inactive.weak_bg_fill = BG_PANEL_ALT;
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        v.widgets.hovered.bg_fill = BG_HOVER;
        v.widgets.hovered.weak_bg_fill = BG_HOVER;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, TEXT3);
        v.widgets.active.bg_fill = BG_ACTIVE;
        v.widgets.active.weak_bg_fill = BG_ACTIVE;
        v.widgets.active.bg_stroke = egui::Stroke::new(1.0, TEXT2);
        v.popup_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(160) };
        // Thin scrollbars that float over content and only show up when needed.
        style.spacing.scroll = egui::style::ScrollStyle::thin();
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.floating_allocated_width = 0.0;
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.interaction.tooltip_delay = 0.6;
    });
}
