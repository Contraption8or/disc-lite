//! Playback through rodio. One track at a time; the app layer decides what
//! plays next.

use std::path::Path;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

pub struct Audio {
    // Playback stops if the device sink is dropped, so it lives here.
    _sink: MixerDeviceSink,
    player: Player,
    /// Length of the loaded track, when the decoder knows it.
    pub duration: Option<f64>,
    loaded: bool,
}

impl Audio {
    pub fn new() -> Result<Self, String> {
        let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        player.pause();
        Ok(Self { _sink: sink, player, duration: None, loaded: false })
    }

    /// Replaces whatever's playing with `path`, starting at `start_fraction`
    /// of the way through.
    pub fn load(&mut self, path: &Path, start_fraction: f64, known_duration: Option<f64>) -> Result<(), String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut builder = Decoder::builder()
            .with_data(std::io::BufReader::new(file))
            .with_byte_len(len)
            .with_seekable(true);
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            builder = builder.with_hint(ext);
        }
        let decoder = builder.build().map_err(|e| e.to_string())?;
        self.duration = decoder
            .total_duration()
            .map(|d| d.as_secs_f64())
            .filter(|d| *d > 0.0)
            .or(known_duration);
        self.player.clear();
        self.player.append(decoder);
        self.loaded = true;
        if start_fraction > 0.0 {
            self.seek_fraction(start_fraction);
        }
        self.player.play();
        Ok(())
    }

    pub fn seek_fraction(&mut self, fraction: f64) {
        if let Some(d) = self.duration {
            let t = (fraction.clamp(0.0, 1.0) * d).max(0.0);
            let _ = self.player.try_seek(Duration::from_secs_f64(t));
        }
    }

    pub fn seek_relative(&mut self, delta: f64) {
        if !self.loaded {
            return;
        }
        let pos = self.position();
        let max = self.duration.unwrap_or(f64::MAX);
        let _ = self.player.try_seek(Duration::from_secs_f64((pos + delta).clamp(0.0, max)));
    }

    pub fn restart(&mut self) {
        let _ = self.player.try_seek(Duration::ZERO);
        self.player.play();
    }

    pub fn play(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    /// The loaded track ran out (vs. merely paused).
    pub fn finished(&self) -> bool {
        self.loaded && self.player.empty()
    }

    /// A track is loaded and hasn't run out (playing or paused).
    pub fn is_active(&self) -> bool {
        self.loaded && !self.player.empty()
    }

    pub fn mark_unloaded(&mut self) {
        self.loaded = false;
    }

    pub fn position(&self) -> f64 {
        self.player.get_pos().as_secs_f64()
    }

    pub fn set_volume(&self, v: f32) {
        self.player.set_volume(v.clamp(0.0, 1.0));
    }
}
