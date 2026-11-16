use crate::theme::{active_palette, ThemePalette};
use eframe::egui::Color32;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[inline]
pub fn c_bg_void() -> Color32 { ThemePalette::c(active_palette().bg_void) }
#[inline]
pub fn c_bg_bar() -> Color32 { ThemePalette::c(active_palette().bg_bar) }
#[inline]
pub fn c_bg_main() -> Color32 { ThemePalette::c(active_palette().bg_main) }
#[inline]
#[inline]
pub fn c_bg_panel() -> Color32 { let p = active_palette(); ThemePalette::ca(p.bg_panel, (p.glass_alpha.clamp(0.25, 1.0) * 255.0) as u8) }
#[inline]
pub fn c_bg_card() -> Color32 { let p = active_palette(); ThemePalette::ca(p.bg_card, (p.glass_alpha.clamp(0.25, 1.0) * 255.0) as u8) }
#[inline]
pub fn c_bg_widget() -> Color32 { let p = active_palette(); ThemePalette::ca(p.bg_widget, (p.glass_alpha.clamp(0.35, 1.0) * 255.0) as u8) } 
#[inline]
pub fn c_bg_hover() -> Color32 { ThemePalette::c(active_palette().bg_hover) }
#[inline]
pub fn c_neon() -> Color32 { ThemePalette::c(active_palette().accent_neon) }
#[inline]
pub fn c_bright() -> Color32 { ThemePalette::c(active_palette().accent_bright) }
#[inline]
pub fn c_deep() -> Color32 { ThemePalette::c(active_palette().accent_deep) }
#[inline]
pub fn c_subtle() -> Color32 { ThemePalette::c(active_palette().accent_subtle) }
#[inline]
pub fn c_border_dark() -> Color32 { ThemePalette::c(active_palette().border_dark) }
#[inline]
pub fn c_border_active() -> Color32 { ThemePalette::c(active_palette().border_active) }
#[inline]
pub fn c_text_pri() -> Color32 { ThemePalette::c(active_palette().text_primary) }
#[inline]
pub fn c_text_muted() -> Color32 { ThemePalette::c(active_palette().text_muted) }
#[inline]
pub fn c_text_gray() -> Color32 { ThemePalette::c(active_palette().text_gray) }
#[inline]
pub fn c_text_dark() -> Color32 { ThemePalette::c(active_palette().text_dark_gray) }

pub const TOKEN_WHITE: Color32 = Color32::from_rgb(235, 240, 248);

pub const CLIENT_ID_A: &str = "23cabbbdc6cd418a";
pub const CLIENT_ID_B: &str = "bb4b39c32c41195d";
pub const SECRET_A: &str = "53bc75238f0c4d08";
pub const SECRET_B: &str = "a118e51fe9203300";
pub const SIGN_SALT_A: &str = "XGRlBW9FXlekgbPr";
pub const SIGN_SALT_B: &str = "RHuSiA";
pub const LYRICS_SALT_A: &str = "p93jhgh689SB";
pub const LYRICS_SALT_B: &str = "ReK6ghtw62";

pub fn app_data_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = PathBuf::from(appdata).join("Ostinato");
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    if let Ok(home) = std::env::var("HOME") {
        let dir = PathBuf::from(home).join(".config").join("Ostinato");
        let _ = fs::create_dir_all(&dir);
        return dir;
    }
    PathBuf::from(".")
}

pub fn config_file_path() -> PathBuf {
    app_data_dir().join("ostinato_config.json")
}

pub fn custom_playlists_path() -> PathBuf {
    app_data_dir().join("custom_playlists.json")
}

pub fn audio_cache_dir() -> PathBuf {
    let dir = app_data_dir().join("cache");
    let _ = fs::create_dir_all(&dir);
    dir
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl Default for RepeatMode {
    fn default() -> Self { Self::Off }
}
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct WaveSettings {
    pub mood_energy: String,
    pub diversity: String,
}

impl Default for WaveSettings {
    fn default() -> Self {
        Self {
            mood_energy: "all".into(),
            diversity: "default".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Keybinds {
    pub play_pause: String,
    pub next_track: String,
    pub prev_track: String,
    pub like_track: String,
    pub shake_wave: String,
    pub sleep_mode: String,
    pub toggle_shuffle: String,
    pub cycle_repeat: String,
    pub vol_up: String,
    pub vol_down: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum TrackProvider {
    Yandex,
    SoundCloud,
    Vk,
    Local,
}

impl Default for TrackProvider {
    fn default() -> Self { Self::Yandex }
}

impl TrackProvider {
    pub fn short_tag(self) -> &'static str {
        match self {
            Self::Yandex => "YM",
            Self::SoundCloud => "SC",
            Self::Vk => "VK",
            Self::Local => "LOC",
        }
    }

    pub fn badge_rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Yandex => (255, 195, 35),
            Self::SoundCloud => (255, 105, 30),
            Self::Vk => (45, 145, 255),
            Self::Local => (130, 160, 190),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct AppConfig {
    pub token: String,  
    pub vk_token: String,
    pub sc_enabled: bool,
    pub local_folder: Option<PathBuf>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub crossfade: bool,
    pub beat_reactive: bool,
    pub active_theme: String,
    pub custom_palette: ThemePalette,
    pub wave_settings: WaveSettings,
    pub keybinds: Keybinds,
    pub wave_recent_ids: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            vk_token: String::new(),
            sc_enabled: true,
            local_folder: None,
            volume: 0.7,
            shuffle: false,
            repeat: RepeatMode::Off,
            crossfade: true,
            beat_reactive: true,
            active_theme: "Neverlose Blue".into(),
            custom_palette: ThemePalette::default(),
            wave_settings: WaveSettings::default(),
            keybinds: Keybinds::default(),
            wave_recent_ids: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_file_path();
        let mut cfg: Self = fs::read_to_string(&path)
            .ok()
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or_default();
        if cfg.volume <= 0.0 && cfg.token.is_empty() && cfg.vk_token.is_empty() {
            cfg.volume = 0.7;
        }
        cfg.volume = cfg.volume.clamp(0.0, 1.0);
        cfg
    }

    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(config_file_path(), json);
        }
    }
}
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct TrackItem {
    pub id: String,
    pub provider: TrackProvider,
    pub title: String,
    pub artist: String,
    pub artist_id: Option<String>,
    pub duration_sec: f32,
    pub liked: bool,
    pub cover_url: Option<String>,
    pub stream_hint: Option<String>,
    pub local_path: Option<PathBuf>,
}

impl Default for TrackItem {
    fn default() -> Self {
        Self {
            id: String::new(),
            provider: TrackProvider::Yandex,
            title: String::new(),
            artist: String::new(),
            artist_id: None,
            duration_sec: 180.0,
            liked: false,
            cover_url: None,
            stream_hint: None,
            local_path: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CustomMixPlaylist {
    pub id: String,
    pub title: String,
    pub tracks: Vec<TrackItem>,
}

pub fn load_custom_playlists() -> Vec<CustomMixPlaylist> {
    fs::read_to_string(custom_playlists_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_custom_playlists(list: &[CustomMixPlaylist]) {
    if let Ok(json) = serde_json::to_string_pretty(list) {
        let _ = fs::write(custom_playlists_path(), json);
    }
}

#[derive(Clone, PartialEq)]
pub enum PlaylistSource {
    Likes,
    UserPlaylist { kind: u64, title: String },
    VkMyAudio,
    CustomMix(String),
}

#[derive(Clone)]
pub struct PlaylistEntry {
    pub title: String,
    pub source: PlaylistSource,
}

#[derive(Clone)]
pub struct ArtistItem { pub id: String, pub name: String }

#[derive(Clone)]
pub struct AlbumItem { pub id: String, pub title: String, pub artist: String, pub year: Option<u32> }

#[derive(Clone)]
pub struct LyricLine { pub time_sec: f32, pub text: String }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlaybackContext { Wave, Playlist, Local, SearchOrArtist }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LeftTab { Wave, Playlists, Search, QueueHistory, LocalFiles, Accounts, Themes, Settings }

#[derive(PartialEq)]
pub enum AppScreen { Onboarding, ReadyTransition(Instant), MainPlayer }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindAction { PlayPause, Next, Prev, Like, ShakeWave, Sleep, ToggleShuffle, CycleRepeat, VolUp, VolDown }

#[derive(Deserialize)]
pub struct DeviceCodeResp {
    pub device_code: Option<String>,
    pub user_code: Option<String>,
    pub verification_url: Option<String>,
    pub interval: Option<u64>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Deserialize)]
pub struct TokenPollResp { pub access_token: Option<String>, pub error: Option<String> }

pub enum AuthMsg { CodeReady { code: String, url: String }, Success(String), Error(String) }

pub enum ApiMsg {
    InitDone { uid: u64, playlists: Vec<PlaylistEntry>, liked_ids: Vec<String>, initial_tracks: Vec<TrackItem> },
    LikesChunkAppended { tracks: Vec<TrackItem> },
    VkAudioLoaded { tracks: Vec<TrackItem> },
    PlaylistLoaded { tracks: Vec<TrackItem> },
    SearchDone { artists: Vec<ArtistItem>, albums: Vec<AlbumItem>, tracks: Vec<TrackItem> },
    ArtistLoaded { artist_name: String, tracks: Vec<TrackItem> },
    AlbumLoaded { album_title: String, tracks: Vec<TrackItem> },
    WaveBatchReady { tracks: Vec<TrackItem>, batch_id: Option<String>, replace_and_play: bool, step_next_immediately: bool },
    AudioReady { seq: u64, track: TrackItem, bytes: Arc<Vec<u8>>, crossfade: bool },
    PreloadReady { track_id: String, bytes: Arc<Vec<u8>> },
    LyricsReady { track_id: String, synced: bool, lines: Vec<LyricLine> },
    ExportDone(String),
    Error(String),
}
