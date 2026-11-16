use crate::models::{audio_cache_dir, TrackItem, TrackProvider};
use crate::win_sys::deep_trim_memory;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::fs;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const CROSSFADE_SEC: f32 = 2.4;
const MAX_CACHE_BYTES: u64 = 500 * 1024 * 1024;

pub enum AudioCmd {
    LoadBytes { seq: u64, bytes: Arc<Vec<u8>>, duration: f32, vol: f32, crossfade: bool },
    LoadFile { seq: u64, path: PathBuf, duration: f32, vol: f32, crossfade: bool },
    Play,
    Pause,
    Seek(f32),
    SetVolume(f32),
}

#[derive(Clone)]
struct SharedBuffer(Arc<Vec<u8>>);
impl AsRef<[u8]> for SharedBuffer { fn as_ref(&self) -> &[u8] { self.0.as_slice() } }

pub struct SharedAudioState {
    pub pos_sec: Mutex<f32>,
    pub playing: AtomicBool,
    pub finished_seq: AtomicU64,
    pub beat_energy: AtomicU32,
}

impl SharedAudioState {
    #[inline]
    pub fn read_beat(&self) -> f32 { f32::from_bits(self.beat_energy.load(Ordering::Relaxed)) }
}

struct SafeBassSource<I> {
    inner: I, lp_state: f32, env_fast: f32, env_slow: f32, counter: u32, state: Arc<SharedAudioState>,
}

impl<I> SafeBassSource<I> {
    fn new(inner: I, state: Arc<SharedAudioState>) -> Self {
        Self { inner, lp_state: 0.0, env_fast: 0.0, env_slow: 0.005, counter: 0, state }
    }
}

impl<I: Source<Item = i16>> Iterator for SafeBassSource<I> {
    type Item = i16;
    #[inline]
    fn next(&mut self) -> Option<i16> {
        let s = self.inner.next()?;
        let val = (s as f32) * (1.0 / 32768.0);
        self.lp_state += 0.025 * (val - self.lp_state);
        let sq = self.lp_state * self.lp_state;
        self.env_fast += 0.01 * (sq - self.env_fast);
        self.env_slow += 0.0005 * (sq - self.env_slow);
        self.counter = self.counter.wrapping_add(1);
        if self.counter & 1023 == 0 {
            let ratio = (self.env_fast / (self.env_slow + 0.00015) - 0.85).max(0.0);
            let kick = (ratio * 0.9 + self.env_fast.sqrt() * 2.4).clamp(0.0, 1.6);
            self.state.beat_energy.store(kick.to_bits(), Ordering::Relaxed);
        }
        Some(s)
    }
}

impl<I: Source<Item = i16>> Source for SafeBassSource<I> {
    #[inline] fn current_frame_len(&self) -> Option<usize> { self.inner.current_frame_len() }
    #[inline] fn channels(&self) -> u16 { self.inner.channels() }
    #[inline] fn sample_rate(&self) -> u32 { self.inner.sample_rate() }
    #[inline] fn total_duration(&self) -> Option<Duration> { self.inner.total_duration() }
    #[inline] fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)
    }
}
struct FadingSink { sink: Sink, elapsed: f32, start_vol: f32 }

pub fn spawn_audio_engine() -> (Sender<AudioCmd>, Arc<SharedAudioState>) {
    let (tx, rx) = mpsc::channel::<AudioCmd>();
    let state = Arc::new(SharedAudioState {
        pos_sec: Mutex::new(0.0), playing: AtomicBool::new(false),
        finished_seq: AtomicU64::new(0), beat_energy: AtomicU32::new(0.0_f32.to_bits()),
    });
    let st_clone = Arc::clone(&state);

    thread::spawn(move || {
        let Ok((_stream, handle)) = OutputStream::try_default() else { return };
        let mut sink: Option<Sink> = None;
        let mut active_seq: u64 = 0;
        let mut fading_old: Option<FadingSink> = None;
        let mut fade_in_elapsed: Option<f32> = None;
        let mut master_vol: f32 = 0.7;
        let mut last_tick = Instant::now();

        loop {
            let is_active = st_clone.playing.load(Ordering::Relaxed) || fading_old.is_some();
            if !is_active {
                st_clone.beat_energy.store(0.0_f32.to_bits(), Ordering::Relaxed);
                if let Ok(first_cmd) = rx.recv() {
                    last_tick = Instant::now();
                    process_audio_cmd(first_cmd, &handle, &mut sink, &mut active_seq, &mut fading_old, &mut fade_in_elapsed, &mut master_vol, &st_clone);
                } else { break; }
            }
            while let Ok(cmd) = rx.try_recv() {
                process_audio_cmd(cmd, &handle, &mut sink, &mut active_seq, &mut fading_old, &mut fade_in_elapsed, &mut master_vol, &st_clone);
            }
            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f32().min(0.25);
            last_tick = now;

            if let Some(mut old) = fading_old.take() {
                old.elapsed += dt;
                if old.elapsed >= CROSSFADE_SEC || old.sink.empty() { old.sink.stop(); }
                else { old.sink.set_volume(old.start_vol * (1.0 - old.elapsed / CROSSFADE_SEC).clamp(0.0, 1.0)); fading_old = Some(old); }
            }
            if let Some(el) = fade_in_elapsed {
                let next_el = el + dt;
                if let Some(s) = &sink {
                    if next_el >= CROSSFADE_SEC { s.set_volume(master_vol); fade_in_elapsed = None; }
                    else { s.set_volume(master_vol * (next_el / CROSSFADE_SEC).clamp(0.0, 1.0)); fade_in_elapsed = Some(next_el); }
                } else { fade_in_elapsed = None; }
            }
            if let Some(s) = &sink {
                if !s.is_paused() && !s.empty() {
                    if let Ok(mut p) = st_clone.pos_sec.lock() { *p = s.get_pos().as_secs_f32(); }
                } else if s.empty() && st_clone.playing.load(Ordering::Relaxed) {
                    st_clone.playing.store(false, Ordering::Relaxed);
                    st_clone.beat_energy.store(0.0_f32.to_bits(), Ordering::Relaxed);
                    st_clone.finished_seq.store(active_seq, Ordering::SeqCst);
                }
            }
            thread::sleep(Duration::from_millis(45));
        }
    });
    (tx, state)
}

fn process_audio_cmd(
    cmd: AudioCmd, handle: &OutputStreamHandle, sink: &mut Option<Sink>, active_seq: &mut u64,
    fading_old: &mut Option<FadingSink>, fade_in_elapsed: &mut Option<f32>, master_vol: &mut f32, st: &Arc<SharedAudioState>,
) {
    match cmd {
        AudioCmd::LoadBytes { seq, bytes, vol, crossfade, .. } => {
            *master_vol = vol.clamp(0.0, 1.0);
            retire_previous_sink(sink, fading_old, crossfade, *master_vol);
            if let Ok(new_sink) = Sink::try_new(handle) {
                let init_v = if crossfade && fading_old.is_some() { *fade_in_elapsed = Some(0.0); 0.02 * *master_vol } else { *fade_in_elapsed = None; *master_vol };
                new_sink.set_volume(init_v);
                if let Ok(source) = Decoder::new(Cursor::new(SharedBuffer(bytes))) {
                    new_sink.append(SafeBassSource::new(source, Arc::clone(st)));
                    new_sink.play();
                    *active_seq = seq;
                    if let Ok(mut p) = st.pos_sec.lock() { *p = 0.0; }
                    st.finished_seq.store(0, Ordering::SeqCst);
                    st.playing.store(true, Ordering::Relaxed);
                    *sink = Some(new_sink);
                }
            }
            deep_trim_memory();
        }
        AudioCmd::LoadFile { seq, path, vol, crossfade, .. } => {
            *master_vol = vol.clamp(0.0, 1.0);
            retire_previous_sink(sink, fading_old, crossfade, *master_vol);
            if let Ok(file) = fs::File::open(path) {
                if let Ok(new_sink) = Sink::try_new(handle) {
                    let init_v = if crossfade && fading_old.is_some() { *fade_in_elapsed = Some(0.0); 0.02 * *master_vol } else { *fade_in_elapsed = None; *master_vol };
                    new_sink.set_volume(init_v);
                    if let Ok(source) = Decoder::new(BufReader::with_capacity(32768, file)) {
                        new_sink.append(SafeBassSource::new(source, Arc::clone(st)));
                        new_sink.play();
                        *active_seq = seq;
                        if let Ok(mut p) = st.pos_sec.lock() { *p = 0.0; }
                        st.finished_seq.store(0, Ordering::SeqCst);
                        st.playing.store(true, Ordering::Relaxed);
                        *sink = Some(new_sink);
                    }
                }
            }
        }
        AudioCmd::Play => { if let Some(s) = sink { s.play(); st.playing.store(true, Ordering::Relaxed); } }
        AudioCmd::Pause => {
            if let Some(old) = fading_old.take() { old.sink.stop(); }
            if let Some(s) = sink { s.pause(); st.playing.store(false, Ordering::Relaxed); st.beat_energy.store(0.0_f32.to_bits(), Ordering::Relaxed); }
        }
        AudioCmd::Seek(sec) => {
            let target = sec.max(0.0);
            st.finished_seq.store(0, Ordering::SeqCst);
            if let Some(s) = sink {
                let _ = s.try_seek(Duration::from_secs_f32(target));
                if let Ok(mut p) = st.pos_sec.lock() { *p = s.get_pos().as_secs_f32(); }
            }
        }
        AudioCmd::SetVolume(v) => { *master_vol = v.clamp(0.0, 1.0); *fade_in_elapsed = None; if let Some(s) = sink { s.set_volume(*master_vol); } }
    }
}

fn retire_previous_sink(sink: &mut Option<Sink>, fading_old: &mut Option<FadingSink>, crossfade: bool, cur_vol: f32) {
    if let Some(prev) = fading_old.take() { prev.sink.stop(); }
    if let Some(old) = sink.take() {
        if crossfade && !old.is_paused() && !old.empty() { *fading_old = Some(FadingSink { sink: old, elapsed: 0.0, start_vol: cur_vol }); }
        else { old.stop(); }
    }
}


pub fn read_cached_mp3(track_id: &str) -> Option<Vec<u8>> {
    if track_id.starts_with("local::") { return None; }
    let safe_id: String = track_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    if safe_id.is_empty() { return None; }
    fs::read(audio_cache_dir().join(format!("{}.audio", safe_id))).ok().filter(|b| b.len() > 16_384)
        .or_else(|| fs::read(audio_cache_dir().join(format!("{}.mp3", safe_id))).ok().filter(|b| b.len() > 16_384))
}

pub fn save_cached_mp3(track_id: &str, bytes: &[u8]) {
    if track_id.starts_with("local::") || bytes.len() < 16_384 { return; }
    let safe_id: String = track_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    if safe_id.is_empty() { return; }
    let dir = audio_cache_dir();
    let tmp_p = dir.join(format!("{}.tmp", safe_id));
    let final_p = dir.join(format!("{}.audio", safe_id));
    if fs::write(&tmp_p, bytes).is_ok() { let _ = fs::rename(&tmp_p, &final_p); enforce_cache_limit(&dir); } else { let _ = fs::remove_file(&tmp_p); }
}

fn enforce_cache_limit(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
    let mut total: u64 = 0;
    for e in entries.flatten() { if let Ok(m) = e.metadata() { if m.is_file() { total += m.len(); files.push((e.path(), m.len(), m.modified().unwrap_or(SystemTime::UNIX_EPOCH))); } } }
    if total <= MAX_CACHE_BYTES { return; }
    files.sort_by_key(|(_, _, t)| *t);
    for (p, len, _) in files { if total <= (MAX_CACHE_BYTES * 80 / 100) { break; } if fs::remove_file(&p).is_ok() { total = total.saturating_sub(len); } }
}

pub fn sanitize_filename(raw: &str) -> String {
    let cl: String = raw.chars().map(|c| match c { '<'|'>'|':'|'"'|'/'|'\\'|'|'|'?'|'*' => '_', c if c.is_control() => ' ', o => o }).collect();
    let tr = cl.trim().trim_matches('.');
    if tr.is_empty() { "Ostinato_Track".into() } else { tr.chars().take(110).collect() }
}

pub fn probe_local_audio_metadata(path: &Path) -> (String, String, f32) {
    use symphonia::core::{formats::FormatOptions, io::MediaSourceStream, meta::{MetadataOptions, StandardTagKey}, probe::Hint};
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Track").to_string();
    let (mut artist, mut title) = stem.split_once(" - ").or_else(|| stem.split_once(" — ")).map(|(a, t)| (a.trim().to_string(), t.trim().to_string())).unwrap_or_else(|| ("Локальный трек".to_string(), stem.clone()));
    let mut dur = 0.0_f32;
    if let Ok(f) = fs::File::open(path) {
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) { hint.with_extension(ext); }
        if let Ok(mut pr) = symphonia::default::get_probe().format(&hint, MediaSourceStream::new(Box::new(f), Default::default()), &FormatOptions::default(), &MetadataOptions::default()) {
            if let Some(t) = pr.format.default_track() { if let (Some(nf), Some(sr)) = (t.codec_params.n_frames, t.codec_params.sample_rate) { if sr > 0 { dur = (nf as f64 / sr as f64) as f32; } } }
            let mut apply = |rev: &symphonia::core::meta::MetadataRevision| {
                for tag in rev.tags() {
                    match tag.std_key {
                        Some(StandardTagKey::TrackTitle) if !tag.value.to_string().trim().is_empty() => title = tag.value.to_string().trim().to_string(),
                        Some(StandardTagKey::Artist) | Some(StandardTagKey::AlbumArtist) if !tag.value.to_string().trim().is_empty() => artist = tag.value.to_string().trim().to_string(),
                        _ => {}
                    }
                }
            };
            if let Some(mg) = pr.metadata.get() { if let Some(r) = mg.current() { apply(r); } }
            if let Some(r) = pr.format.metadata().current() { apply(r); }
        }
    }
    if dur < 1.0 { dur = fs::metadata(path).map(|m| (m.len() as f32 / 24_000.0).clamp(10.0, 3600.0)).unwrap_or(180.0); }
    (title, artist, dur)
}

pub fn scan_folder_tracks(folder: &Path) -> Vec<TrackItem> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(folder) else { return out };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|e| e.to_str()).map(|x| matches!(x.to_ascii_lowercase().as_str(), "mp3"|"flac"|"wav"|"ogg"|"m4a")).unwrap_or(false)).collect();
    paths.sort();
    for path in paths {
        let (title, artist, duration_sec) = probe_local_audio_metadata(&path);
        out.push(TrackItem { id: format!("local::{}", path.to_string_lossy()), provider: TrackProvider::Local, title, artist, artist_id: None, duration_sec, liked: false, cover_url: None, stream_hint: None, local_path: Some(path) });
    }
    out
}
