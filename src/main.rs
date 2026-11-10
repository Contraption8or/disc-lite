#![windows_subsystem = "windows"]
// egui 0.37 renamed a few things (Id::new, Ui::id, ...); the old names still work.
#![allow(deprecated)]

mod app;
mod audio;
mod dragout;
mod icons;
mod library;
mod migrate;
mod model;
mod persist;
mod theme;
mod tree;
mod ui_player;
mod ui_sidebar;
mod ui_tracks;
mod waveform;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Disc Lite")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([760.0, 480.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/disc-icon-64.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("Disc Lite", options, Box::new(|cc| Ok(Box::new(app::DiscApp::new(cc)))))
}
