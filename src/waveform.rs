//! Waveform peaks: decoded on background threads with symphonia, kept in
//! memory, and saved to a small cache file so reopening the app shows every
//! waveform instantly instead of decoding again.
//!
//! Left and right are kept separately so the drawing can show panning: left
//! above the center line, right below it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::UNIX_EPOCH;

use eframe::egui;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Resolution stored per track (per channel); rows resample this to their
/// width in pixels.
pub const BUCKETS: usize = 1600;

/// Peak level per slice for each side. Mono files have left == right.
pub struct Peaks {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub duration: f64,
}

#[derive(Clone)]
pub enum Wave {
    Pending,
    Ready(Arc<Peaks>),
    Failed,
}

struct Job {
    id: String,
    path: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Stamp {
    size: u64,
    mtime: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(Stamp { size: meta.len(), mtime })
}

#[derive(Default)]
struct DiskCache {
    entries: HashMap<String, (Stamp, Arc<Peaks>)>,
    dirty: bool,
}

pub struct Waveforms {
    state: Arc<Mutex<HashMap<String, Wave>>>,
    queue: Arc<(Mutex<Vec<Job>>, Condvar)>,
    disk: Arc<Mutex<DiskCache>>,
    cache_path: PathBuf,
}

impl Waveforms {
    pub fn new(ctx: egui::Context, cache_path: PathBuf) -> Self {
        let state: Arc<Mutex<HashMap<String, Wave>>> = Arc::default();
        let queue: Arc<(Mutex<Vec<Job>>, Condvar)> = Arc::default();
        let disk = Arc::new(Mutex::new(load_cache(&cache_path)));

        let workers = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1))
            .unwrap_or(3)
            .clamp(2, 8);
        for _ in 0..workers {
            let (state, queue, disk, ctx) = (state.clone(), queue.clone(), disk.clone(), ctx.clone());
            std::thread::spawn(move || loop {
                let job = {
                    let (lock, cvar) = &*queue;
                    let mut q = lock.lock().unwrap();
                    while q.is_empty() {
                        q = cvar.wait(q).unwrap();
                    }
                    // Newest request first: whatever just scrolled into view.
                    q.pop().unwrap()
                };
                let st = stamp(&job.path);
                let cached = st.and_then(|s| {
                    let d = disk.lock().unwrap();
                    d.entries.get(&job.id).filter(|(cs, _)| *cs == s).map(|(_, p)| p.clone())
                });
                let result = match cached {
                    Some(p) => Wave::Ready(p),
                    None => match decode_peaks(&job.path) {
                        Some(p) => {
                            let p = Arc::new(p);
                            if let Some(s) = st {
                                let mut d = disk.lock().unwrap();
                                d.entries.insert(job.id.clone(), (s, p.clone()));
                                d.dirty = true;
                            }
                            Wave::Ready(p)
                        }
                        None => Wave::Failed,
                    },
                };
                state.lock().unwrap().insert(job.id, result);
                ctx.request_repaint();
            });
        }
        Self { state, queue, disk, cache_path }
    }

    /// Current state for a track, queueing a decode the first time it's
    /// asked for. Cheap enough to call for every visible row every frame.
    pub fn get(&self, id: &str, path: &Path) -> Wave {
        let mut st = self.state.lock().unwrap();
        if let Some(w) = st.get(id) {
            return w.clone();
        }
        st.insert(id.to_string(), Wave::Pending);
        drop(st);
        let (lock, cvar) = &*self.queue;
        lock.lock().unwrap().push(Job { id: id.to_string(), path: path.to_path_buf() });
        cvar.notify_one();
        Wave::Pending
    }

    /// Already-decoded duration, if known (no decode is queued by this).
    pub fn duration(&self, id: &str) -> Option<f64> {
        match self.state.lock().unwrap().get(id) {
            Some(Wave::Ready(p)) => Some(p.duration),
            _ => None,
        }
    }

    /// Forget a track's waveform (its file changed or was renamed).
    pub fn invalidate(&self, id: &str) {
        self.state.lock().unwrap().remove(id);
    }

    pub fn save_cache(&self) {
        let mut d = self.disk.lock().unwrap();
        if !d.dirty {
            return;
        }
        if write_cache(&self.cache_path, &d).is_ok() {
            d.dirty = false;
        }
    }
}

/// Decodes the whole file and keeps the loudest sample of each slice, for
/// each side.
fn decode_peaks(path: &Path) -> Option<Peaks> {
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44_100) as u64;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    // Peak per 2ms block while decoding; downsampled to BUCKETS at the end.
    // Fine enough that short sound effects still get real detail.
    let block = (sample_rate / 500).max(1);
    let mut blocks_l: Vec<f32> = Vec::with_capacity(100_000);
    let mut blocks_r: Vec<f32> = Vec::with_capacity(100_000);
    let (mut max_l, mut max_r, mut in_block, mut total_frames) = (0f32, 0f32, 0u64, 0u64);
    let mut buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let cap = decoded.capacity() as u64;
        if buf.as_ref().is_none_or(|b| b.capacity() < cap as usize * channels) {
            buf = Some(SampleBuffer::<f32>::new(cap, spec));
        }
        let b = buf.as_mut().unwrap();
        b.copy_interleaved_ref(decoded);
        for frame in b.samples().chunks_exact(channels) {
            let l = frame[0].abs();
            let r = if channels > 1 { frame[1].abs() } else { l };
            max_l = max_l.max(l);
            max_r = max_r.max(r);
            in_block += 1;
            total_frames += 1;
            if in_block == block {
                blocks_l.push(max_l);
                blocks_r.push(max_r);
                max_l = 0.0;
                max_r = 0.0;
                in_block = 0;
            }
        }
    }
    if in_block > 0 {
        blocks_l.push(max_l);
        blocks_r.push(max_r);
    }
    if blocks_l.is_empty() {
        return None;
    }
    Some(Peaks {
        left: downsample(&blocks_l),
        right: downsample(&blocks_r),
        duration: total_frames as f64 / sample_rate as f64,
    })
}

fn downsample(blocks: &[f32]) -> Vec<f32> {
    let n = blocks.len();
    (0..BUCKETS)
        .map(|i| {
            let start = (i * n / BUCKETS).min(n - 1);
            let end = ((i + 1) * n / BUCKETS).max(start + 1).min(n);
            blocks[start..end].iter().copied().fold(0.0, f32::max).min(1.0)
        })
        .collect()
}

// --- Cache file -------------------------------------------------------------
// [magic "DLW2"] then per entry:
//   path_len u32, path utf8, size u64, mtime u64, duration f64,
//   left [u8; BUCKETS], right [u8; BUCKETS]
// An older cache (different magic) is ignored and rebuilt.

const MAGIC: &[u8; 4] = b"DLW2";

// Stored on a square-root curve so quiet detail keeps more of the 256 steps.
fn pack(v: f32) -> u8 {
    (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8
}

fn unpack(b: &[u8]) -> Vec<f32> {
    b.iter().map(|&x| (x as f32 / 255.0).powi(2)).collect()
}

fn load_cache(path: &Path) -> DiskCache {
    let mut cache = DiskCache::default();
    let Ok(mut f) = std::fs::File::open(path) else { return cache };
    let mut bytes = Vec::new();
    if f.read_to_end(&mut bytes).is_err() || bytes.len() < 4 || &bytes[..4] != MAGIC {
        return cache;
    }
    let mut i = 4;
    let take = |i: &mut usize, n: usize| -> Option<&[u8]> {
        let s = bytes.get(*i..*i + n)?;
        *i += n;
        Some(s)
    };
    while i < bytes.len() {
        let Some(len) = take(&mut i, 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize) else { break };
        let Some(id) = take(&mut i, len).and_then(|b| std::str::from_utf8(b).ok()).map(String::from) else { break };
        let Some(size) = take(&mut i, 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())) else { break };
        let Some(mtime) = take(&mut i, 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())) else { break };
        let Some(duration) = take(&mut i, 8).map(|b| f64::from_le_bytes(b.try_into().unwrap())) else { break };
        let Some(left) = take(&mut i, BUCKETS).map(unpack) else { break };
        let Some(right) = take(&mut i, BUCKETS).map(unpack) else { break };
        cache.entries.insert(id, (Stamp { size, mtime }, Arc::new(Peaks { left, right, duration })));
    }
    cache
}

fn write_cache(path: &Path, cache: &DiskCache) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = Vec::with_capacity(cache.entries.len() * (BUCKETS * 2 + 128));
    out.extend_from_slice(MAGIC);
    for (id, (st, p)) in &cache.entries {
        out.extend_from_slice(&(id.len() as u32).to_le_bytes());
        out.extend_from_slice(id.as_bytes());
        out.extend_from_slice(&st.size.to_le_bytes());
        out.extend_from_slice(&st.mtime.to_le_bytes());
        out.extend_from_slice(&p.duration.to_le_bytes());
        out.extend(p.left.iter().map(|v| pack(*v)));
        out.extend(p.right.iter().map(|v| pack(*v)));
    }
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::File::create(&tmp)?;
    f.write_all(&out)?;
    drop(f);
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a 1s 16-bit stereo WAV: a tone in the left channel only for
    /// the first half, then in the right channel only.
    fn write_panned_wav(path: &Path) {
        let rate = 44_100u32;
        let mut data = Vec::new();
        for i in 0..rate {
            let s = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 20_000.0) as i16;
            let (l, r) = if i < rate / 2 { (s, 0) } else { (0, s) };
            data.extend_from_slice(&l.to_le_bytes());
            data.extend_from_slice(&r.to_le_bytes());
        }
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes()); // PCM
        f.extend_from_slice(&2u16.to_le_bytes()); // stereo
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * 4).to_le_bytes());
        f.extend_from_slice(&4u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(&data);
        std::fs::write(path, f).unwrap();
    }

    #[test]
    fn keeps_left_and_right_apart() {
        let path = std::env::temp_dir().join("disc-lite-pan-test.wav");
        write_panned_wav(&path);
        let p = decode_peaks(&path).expect("decodes");
        let _ = std::fs::remove_file(&path);
        let (a, b) = (BUCKETS / 4, BUCKETS * 3 / 4);
        assert!(p.left[a] > 0.5 && p.right[a] < 0.01, "first half is left-only");
        assert!(p.right[b] > 0.5 && p.left[b] < 0.01, "second half is right-only");
        assert!((p.duration - 1.0).abs() < 0.01);
        // Survives a cache round trip.
        let back = unpack(&p.left.iter().map(|v| pack(*v)).collect::<Vec<_>>());
        assert!((back[a] - p.left[a]).abs() < 0.02);
    }
}
