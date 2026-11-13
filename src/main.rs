#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, FontId, RichText, Rounding, Sense, Stroke, Vec2};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use md5::{Digest, Md5};
use rodio::{Decoder, OutputStream, Sink};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use image as _;

const CONFIG_FILE: &str = "ostinato_config.json";

const CLIENT_ID_A: &str = "23cabbbdc6cd418a";
const CLIENT_ID_B: &str = "bb4b39c32c41195d";
const SECRET_A: &str = "53bc75238f0c4d08";
const SECRET_B: &str = "a118e51fe9203300";
const SIGN_SALT_A: &str = "XGRlBW9FXlekgbPr";
const SIGN_SALT_B: &str = "RHuSiA";

const BG_VOID: Color32 = Color32::from_rgb(3, 5, 9);
const BG_BAR: Color32 = Color32::from_rgb(5, 8, 14);
const BG_PANEL: Color32 = Color32::from_rgb(8, 12, 20);
const BG_MAIN: Color32 = Color32::from_rgb(6, 9, 16);
const BG_WIDGET: Color32 = Color32::from_rgb(11, 17, 28);
const BG_HOVER: Color32 = Color32::from_rgb(16, 26, 44);

const BLUE_NEON: Color32 = Color32::from_rgb(0, 135, 250);
const BLUE_BRIGHT: Color32 = Color32::from_rgb(55, 165, 255);
const BLUE_DEEP: Color32 = Color32::from_rgb(0, 68, 135);
const BORDER_DARK_BLUE: Color32 = Color32::from_rgb(16, 28, 48);

const TEXT_BLUE: Color32 = Color32::from_rgb(120, 175, 230);
const TEXT_GRAY: Color32 = Color32::from_rgb(85, 100, 120);
#[derive(Serialize, Deserialize, Clone)]
struct Keybinds {
    play_pause: String,
    next_track: String,
    prev_track: String,
    like_track: String,
    shake_wave: String,
    super_sleep: String,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            play_pause: "Ctrl+Alt+Space".into(),
            next_track: "Ctrl+Alt+Right".into(),
            prev_track: "Ctrl+Alt+Left".into(),
            like_track: "Ctrl+Alt+L".into(),
            shake_wave: "Ctrl+Alt+W".into(),
            super_sleep: "Ctrl+Alt+S".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct AppConfig {
    token: String,
    local_folder: Option<PathBuf>,
    volume: f32,
    keybinds: Keybinds,
}

#[derive(Clone, PartialEq)]
enum PlaylistSource {
    Wave,
    Likes,
    UserPlaylist { kind: u64, title: String },
}

#[derive(Clone)]
struct PlaylistEntry {
    title: String,
    source: PlaylistSource,
}

#[derive(Clone)]
struct ArtistItem {
    id: String,
    name: String,
}

#[derive(Clone)]
struct TrackItem {
    id: String,
    title: String,
    artist: String,
    artist_id: Option<String>,
    duration_sec: f32,
    liked: bool,
    cover_url: Option<String>,
    local_path: Option<PathBuf>,
}


#[derive(Deserialize)]
struct DeviceCodeResp {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_url: Option<String>,
    interval: Option<u64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct TokenPollResp {
    access_token: Option<String>,
    error: Option<String>,
}
enum AuthMsg {
    CodeReady { code: String, url: String },
    Success(String),
    Error(String),
}

enum ApiMsg {
    InitDone {
        uid: u64,
        playlists: Vec<PlaylistEntry>,
        liked_ids: Vec<String>,
        initial_tracks: Vec<TrackItem>,
    },
    TracksLoaded(Vec<TrackItem>),
    SearchDone {
        artists: Vec<ArtistItem>,
        tracks: Vec<TrackItem>,
    },
    ArtistLoaded {
        artist_name: String,
        tracks: Vec<TrackItem>,
    },
    WaveAppended {
        tracks: Vec<TrackItem>,
        auto_play_next: bool,
    },
    AudioReady {
        seq: u64,
        track: TrackItem,
        bytes: Arc<Vec<u8>>,
    },
    Error(String),
}


#[derive(Clone, Copy, PartialEq, Eq)]
enum BindAction {
    PlayPause,
    Next,
    Prev,
    Like,
    ShakeWave,
    SuperSleep,
}

enum AudioCmd {
    LoadBytes(Arc<Vec<u8>>, f32),
    LoadFile(PathBuf, f32),
    Play,
    Pause,
    Seek(f32),
    SetVolume(f32),
}

#[derive(Clone)]
struct SharedBuffer(Arc<Vec<u8>>);

impl AsRef<[u8]> for SharedBuffer {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

struct SharedAudioState {
    pos_sec: Mutex<f32>,
    playing: AtomicBool,
    finished: AtomicBool,
}
fn spawn_audio_engine() -> (Sender<AudioCmd>, Arc<SharedAudioState>) {
    let (tx, rx) = mpsc::channel::<AudioCmd>();
    let state = Arc::new(SharedAudioState {
        pos_sec: Mutex::new(0.0),
        playing: AtomicBool::new(false),
        finished: AtomicBool::new(false),
    });
    let st_clone = Arc::clone(&state);

    thread::spawn(move || {
        let Ok((_stream, handle)) = OutputStream::try_default() else {
            return;
        };

        let mut sink: Option<Sink> = None;
        let mut last_tick = Instant::now();

        loop {
            let is_active = st_clone.playing.load(Ordering::Relaxed);
            if !is_active {
                if let Ok(first_cmd) = rx.recv() {
                    last_tick = Instant::now();
                    process_audio_cmd(first_cmd, &handle, &mut sink, &st_clone);
                } else {
                    break;
                }
            }

            while let Ok(cmd) = rx.try_recv() {
                process_audio_cmd(cmd, &handle, &mut sink, &st_clone);
            }

            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f32();
            last_tick = now;

            if let Some(s) = &sink {
                if !s.is_paused() && !s.empty() {
                    if let Ok(mut p) = st_clone.pos_sec.lock() {
                        *p += dt;
                    }
                } else if s.empty() && st_clone.playing.load(Ordering::Relaxed) {
                    st_clone.playing.store(false, Ordering::Relaxed);
                    st_clone.finished.store(true, Ordering::Relaxed);
                }
            }

            thread::sleep(Duration::from_millis(50));
        }
    });

    (tx, state)
}

fn process_audio_cmd(
    cmd: AudioCmd,
    handle: &rodio::OutputStreamHandle,
    sink: &mut Option<Sink>,
    st: &Arc<SharedAudioState>,
) {
    match cmd {
        AudioCmd::LoadBytes(bytes, vol) => {
            if let Some(old) = sink.take() {
                old.stop();
            }
            if let Ok(new_sink) = Sink::try_new(handle) {
                new_sink.set_volume(vol.clamp(0.0, 1.0));
                let cursor = Cursor::new(SharedBuffer(bytes));
                if let Ok(source) = Decoder::new(cursor) {
                    new_sink.append(source);
                    new_sink.play();
                    if let Ok(mut p) = st.pos_sec.lock() {
                        *p = 0.0;
                    }
                    st.finished.store(false, Ordering::Relaxed);
                    st.playing.store(true, Ordering::Relaxed);
                    *sink = Some(new_sink);
                }
            }
            deep_trim_memory();
        }
        AudioCmd::LoadFile(path, vol) => {
            if let Some(old) = sink.take() {
                old.stop();
            }
            if let Ok(file) = fs::File::open(path) {
                if let Ok(new_sink) = Sink::try_new(handle) {
                    new_sink.set_volume(vol.clamp(0.0, 1.0));
                    if let Ok(source) = Decoder::new(std::io::BufReader::with_capacity(32768, file)) {
                        new_sink.append(source);
                        new_sink.play();
                        if let Ok(mut p) = st.pos_sec.lock() {
                            *p = 0.0;
                        }
                        st.finished.store(false, Ordering::Relaxed);
                        st.playing.store(true, Ordering::Relaxed);
                        *sink = Some(new_sink);
                    }
                }
            }
        }
        AudioCmd::Play => {
            if let Some(s) = sink {
                s.play();
                st.playing.store(true, Ordering::Relaxed);
            }
        }
        AudioCmd::Pause => {
            if let Some(s) = sink {
                s.pause();
                st.playing.store(false, Ordering::Relaxed);
            }
        }
        AudioCmd::Seek(sec) => {
            let target = sec.max(0.0);
            if let Some(s) = sink {
                if s.try_seek(Duration::from_secs_f32(target)).is_ok() {
                    if let Ok(mut p) = st.pos_sec.lock() {
                        *p = target;
                    }
                }
            }
        }
        AudioCmd::SetVolume(v) => {
            if let Some(s) = sink {
                s.set_volume(v.clamp(0.0, 1.0));
            }
        }
    }
}
fn build_ostinato_tray() -> (Option<TrayIcon>, tray_icon::menu::MenuId, tray_icon::menu::MenuId) {
    let menu = Menu::new();
    let item_show = MenuItem::new("Развернуть Ostinato", true, None);
    let item_quit = MenuItem::new("Выйти полностью", true, None);
    let show_id = item_show.id().clone();
    let quit_id = item_quit.id().clone();
    let _ = menu.append(&item_show);
    let _ = menu.append(&item_quit);

    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let idx = (y * 32 + x) * 4;
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 14.5 {
                if (dist - 10.0).abs() <= 2.3 || dist <= 3.2 {
                    rgba[idx] = 0;
                    rgba[idx + 1] = 145;
                    rgba[idx + 2] = 255;
                    rgba[idx + 3] = 255;
                } else {
                    rgba[idx] = 5;
                    rgba[idx + 1] = 9;
                    rgba[idx + 2] = 16;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }

    let icon = tray_icon::Icon::from_rgba(rgba, 32, 32).ok();
    let mut builder = TrayIconBuilder::new()
        .with_tooltip("Ostinato — ЛКМ: Развернуть | ПКМ: Выход")
        .with_menu(Box::new(menu));
    if let Some(ic) = icon {
        builder = builder.with_icon(ic);
    }

    (builder.build().ok(), show_id, quit_id)
}

static MAIN_HWND: AtomicU64 = AtomicU64::new(0);

#[cfg(windows)]
#[cfg(windows)]
unsafe extern "system" fn enum_win_proc(
    hwnd: *mut std::ffi::c_void,
    lparam: isize,
) -> i32 {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    };

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == lparam as u32 && IsWindowVisible(hwnd) != 0 {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) != 0 {
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            // Отсекаем невидимые 0x0 окна трея — берем ТОЛЬКО реальное окно плеера!
            if width > 300 && height > 200 {
                MAIN_HWND.store(hwnd as usize as u64, Ordering::Relaxed);
                return 0;
            }
        }
    }
    1
}


fn capture_own_hwnd() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;
        if MAIN_HWND.load(Ordering::Relaxed) != 0 {
            return;
        }
        let my_pid = GetCurrentProcessId();
        EnumWindows(Some(enum_win_proc), my_pid as isize);
    }
}

fn force_hide_window_win32() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetWindowLongW, SetWindowPos, ShowWindow, GWL_EXSTYLE,
            SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE,
            WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        };
        if MAIN_HWND.load(Ordering::Relaxed) == 0 {
            capture_own_hwnd();
        }
        let raw = MAIN_HWND.load(Ordering::Relaxed);
        if raw != 0 {
            let hwnd = raw as usize as *mut std::ffi::c_void;
            // Прячем из Alt+Tab и панели задач, но оставляем живой прием хоткеев!
            ShowWindow(hwnd, SW_HIDE);
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let new_ex = (ex & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW;
            SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                -32000,
                -32000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

fn force_wake_window_win32() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetForegroundWindow, SetWindowLongW, SetWindowPos, ShowWindow,
            GWL_EXSTYLE, SWP_NOSIZE, SWP_NOZORDER, SW_RESTORE, SW_SHOW, WS_EX_APPWINDOW,
            WS_EX_TOOLWINDOW,
        };
        let raw = MAIN_HWND.load(Ordering::Relaxed);
        if raw != 0 {
            let hwnd = raw as usize as *mut std::ffi::c_void;
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            let new_ex = (ex & !WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW;
            SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                180,
                140,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER,
            );
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}



fn set_process_eco_qos(enable: bool) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
            BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
        };

        let handle = GetCurrentProcess();
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if enable {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
        };

        let _ = SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            &state as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );

        let prio = if enable {
            BELOW_NORMAL_PRIORITY_CLASS
        } else {
            NORMAL_PRIORITY_CLASS
        };
        SetPriorityClass(handle, prio);
    }
}

fn deep_trim_memory() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Memory::SetProcessWorkingSetSizeEx;
        use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let handle = GetCurrentProcess();
        let _ = SetProcessWorkingSetSizeEx(handle, usize::MAX, usize::MAX, 0);
        K32EmptyWorkingSet(handle);
    }
}
fn ym_api_base() -> String {
    format!("https://{}.{}.net", "api.music", "yandex")
}

fn shared_ym_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            let host = attempt.url().host_str().unwrap_or("");
            if host.contains("dzen.ru") || host.contains("sso.") {
                attempt.stop()
            } else if attempt.previous().len() > 5 {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        reqwest::blocking::Client::builder()
            .redirect(policy)
            .user_agent("Yandex-Music-API")
            .timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(2)
            .build()
            .unwrap_or_default()
    })
}

fn parse_track_value(v: &Value, liked_set: &[String]) -> Option<TrackItem> {
    let t = if v.get("track").is_some() { &v["track"] } else { v };
    if t["available"].as_bool() == Some(false) {
        return None;
    }
    let id = match &t["id"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let title = t["title"].as_str().unwrap_or("Без названия").to_string();

    let mut artist_id: Option<String> = None;
    let mut artists = Vec::new();
    if let Some(arr) = t["artists"].as_array() {
        for a in arr {
            if artist_id.is_none() {
                artist_id = match &a["id"] {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                };
            }
            if let Some(name) = a["name"].as_str() {
                artists.push(name.to_string());
            }
        }
    }

    let artist = if artists.is_empty() {
        "Неизвестен".to_string()
    } else {
        artists.join(", ")
    };
    let dur_ms = t["durationMs"].as_f64().unwrap_or(180_000.0).max(1000.0);
    let liked = liked_set.iter().any(|x| x == &id);
    let cover_url = t["coverUri"]
        .as_str()
        .or_else(|| t["ogImage"].as_str())
        .map(|u| format!("https://{}", u.replace("%%", "300x300")));

    Some(TrackItem {
        id,
        title,
        artist,
        artist_id,
        duration_sec: (dur_ms / 1000.0) as f32,
        liked,
        cover_url,
        local_path: None,
    })
}

fn search_ym_all(
    token: &str,
    query: &str,
    liked_set: &[String],
) -> Result<(Vec<ArtistItem>, Vec<TrackItem>), String> {
    let client = shared_ym_client();
    let url = format!("{}/search", ym_api_base());
    let auth = format!("OAuth {}", token);

    let v: Value = client
        .get(&url)
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .query(&[
            ("text", query),
            ("type", "all"),
            ("page", "0"),
            ("nococrrect", "false"),
        ])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let mut artists = Vec::new();
    if let Some(arr) = v["result"]["artists"]["results"].as_array() {
        for a in arr.iter().take(4) {
            let aid = match &a["id"] {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            };
            if let (Some(id), Some(name)) = (aid, a["name"].as_str()) {
                artists.push(ArtistItem {
                    id,
                    name: name.to_string(),
                });
            }
        }
    }

    let mut tracks = Vec::new();
    if let Some(arr) = v["result"]["tracks"]["results"].as_array() {
        for item in arr {
            if let Some(tr) = parse_track_value(item, liked_set) {
                tracks.push(tr);
            }
        }
    }

    Ok((artists, tracks))
}

fn fetch_artist_tracks(
    token: &str,
    artist_id: &str,
    liked_set: &[String],
) -> Result<Vec<TrackItem>, String> {
    let client = shared_ym_client();
    let url = format!("{}/artists/{}/tracks", ym_api_base(), artist_id);
    let auth = format!("OAuth {}", token);

    let v: Value = client
        .get(&url)
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .query(&[("page", "0"), ("page-size", "60")])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    if let Some(arr) = v["result"]["tracks"].as_array() {
        for item in arr {
            if let Some(tr) = parse_track_value(item, liked_set) {
                out.push(tr);
            }
        }
    }
    Ok(out)
}


fn fetch_ym_init(
    token: &str,
) -> Result<(u64, Vec<PlaylistEntry>, Vec<String>, Vec<TrackItem>), String> {
    let client = shared_ym_client();
    let base = ym_api_base();
    let auth = format!("OAuth {}", token);

    let st_resp: Value = client
        .get(format!("{}/account/status", base))
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let uid = st_resp["result"]["account"]["uid"]
        .as_u64()
        .ok_or_else(|| "Неверный токен или не найден UID аккаунта".to_string())?;

    let mut playlists = vec![
        PlaylistEntry {
            title: "Моя волна".into(),
            source: PlaylistSource::Wave,
        },
        PlaylistEntry {
            title: "Мне нравится".into(),
            source: PlaylistSource::Likes,
        },
    ];

    if let Ok(pl_r) = client
        .get(format!("{}/users/{}/playlists/list", base, uid))
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .send()
    {
        if let Ok(pl_v) = pl_r.json::<Value>() {
            if let Some(arr) = pl_v["result"].as_array() {
                for item in arr {
                    if let (Some(kind), Some(title)) =
                        (item["kind"].as_u64(), item["title"].as_str())
                    {
                        playlists.push(PlaylistEntry {
                            title: title.to_string(),
                            source: PlaylistSource::UserPlaylist {
                                kind,
                                title: title.to_string(),
                            },
                        });
                    }
                }
            }
        }
    }

    let likes_v: Value = client
        .get(format!("{}/users/{}/likes/tracks", base, uid))
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let mut liked_ids = Vec::new();
    if let Some(arr) = likes_v["result"]["library"]["tracks"].as_array() {
        for item in arr {
            let tid = match &item["id"] {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            };
            if let Some(id_str) = tid {
                liked_ids.push(id_str);
            }
        }
    }

    let initial_tracks = fetch_tracks_by_ids(client, &base, &auth, &liked_ids, &liked_ids)?;
    Ok((uid, playlists, liked_ids, initial_tracks))
}
fn fetch_tracks_by_ids(
    client: &reqwest::blocking::Client,
    base: &str,
    auth: &str,
    ids: &[String],
    liked_set: &[String],
) -> Result<Vec<TrackItem>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let slice: Vec<&str> = ids.iter().take(150).map(|s| s.as_str()).collect();
    let joined = slice.join(",");

    let v: Value = client
        .post(format!("{}/tracks", base))
        .header("Authorization", auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .form(&[("track-ids", joined.as_str())])
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    if let Some(arr) = v["result"].as_array() {
        for item in arr {
            if let Some(mut tr) = parse_track_value(item, liked_set) {
                tr.liked = liked_set.iter().any(|x| x == &tr.id);
                out.push(tr);
            }
        }
    }
    Ok(out)
}

fn fetch_wave_batch(
    token: &str,
    liked_set: &[String],
    last_track_id: Option<&str>,
) -> Result<Vec<TrackItem>, String> {
    let client = shared_ym_client();
    let base = ym_api_base();
    let auth = format!("OAuth {}", token);

    if let Some(prev_id) = last_track_id {
        if !prev_id.is_empty() {
            let fb_url = format!("{}/rotor/station/user:onyourwave/feedback", base);
            let now_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let fb_body = serde_json::json!({
                "type": "trackFinished",
                "timestamp": now_ts,
                "trackId": prev_id,
                "totalPlayedSeconds": 120
            });
            let _ = client
                .post(&fb_url)
                .header("Authorization", &auth)
                .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
                .json(&fb_body)
                .send();
        }
    }

    let url = match last_track_id {
        Some(q) if !q.is_empty() => format!(
            "{}/rotor/station/user:onyourwave/tracks?settings2=true&queue={}",
            base, q
        ),
        _ => format!("{}/rotor/station/user:onyourwave/tracks?settings2=true", base),
    };

    let v: Value = client
        .get(&url)
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    if let Some(seq) = v["result"]["sequence"].as_array() {
        for item in seq {
            if let Some(tr) = parse_track_value(&item["track"], liked_set) {
                out.push(tr);
            }
        }
    }
    Ok(out)
}

fn fetch_playlist_tracks(
    token: &str,
    uid: u64,
    src: &PlaylistSource,
    liked_set: &[String],
) -> Result<Vec<TrackItem>, String> {
    let client = shared_ym_client();
    let base = ym_api_base();
    let auth = format!("OAuth {}", token);

    match src {
        PlaylistSource::Likes => fetch_tracks_by_ids(client, &base, &auth, liked_set, liked_set),
        PlaylistSource::UserPlaylist { kind, .. } => {
            let v: Value = client
                .get(format!("{}/users/{}/playlists/{}", base, uid, kind))
                .header("Authorization", &auth)
                .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
                .send()
                .map_err(|e| e.to_string())?
                .json()
                .map_err(|e| e.to_string())?;
            let mut out = Vec::new();
            if let Some(arr) = v["result"]["tracks"].as_array() {
                for item in arr {
                    if let Some(tr) = parse_track_value(item, liked_set) {
                        out.push(tr);
                    }
                }
            }
            Ok(out)
        }
        PlaylistSource::Wave => fetch_wave_batch(token, liked_set, None),
    }
}

fn download_track_mp3(token: &str, track_id: &str) -> Result<Vec<u8>, String> {
    let client = shared_ym_client();
    let base = ym_api_base();
    let auth = format!("OAuth {}", token);

    let info_v: Value = client
        .get(format!("{}/tracks/{}/download-info", base, track_id))
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let arr = info_v["result"]
        .as_array()
        .ok_or_else(|| "Нет доступных потоков для трека".to_string())?;

    let best = arr
        .iter()
        .filter(|x| x["codec"].as_str() == Some("mp3"))
        .max_by_key(|x| x["bitrateInKbps"].as_u64().unwrap_or(0))
        .or_else(|| arr.first())
        .ok_or_else(|| "Пустой список аудиопотоков".to_string())?;

    let dl_url = best["downloadInfoUrl"]
        .as_str()
        .ok_or_else(|| "Отсутствует ссылка downloadInfoUrl".to_string())?;

    let sep = if dl_url.contains('?') { "&" } else { "?" };
    let xml_info = client
        .get(format!("{}{}format=json", dl_url, sep))
        .header("Authorization", &auth)
        .send()
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .map_err(|e| e.to_string())?;

    let host = xml_info["host"].as_str().ok_or("Пустой host потока")?;
    let path = xml_info["path"].as_str().ok_or("Пустой path потока")?;
    let ts = xml_info["ts"].as_str().ok_or("Пустой ts потока")?;
    let s = xml_info["s"].as_str().ok_or("Пустой s потока")?;

    let salt = format!("{}{}", SIGN_SALT_A, SIGN_SALT_B);
    let path_sub = path.strip_prefix('/').unwrap_or(path);
    let mut hasher = Md5::new();
    hasher.update(format!("{}{}{}", salt, path_sub, s).as_bytes());
    let sign = format!("{:x}", hasher.finalize());

    let direct_url = format!("https://{}/get-mp3/{}/{}{}", host, sign, ts, path);
    let bytes = client
        .get(&direct_url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;

    Ok(bytes.to_vec())
}
fn parse_hotkey_str(combo: &str) -> Option<HotKey> {
    let mut mods = Modifiers::empty();
    let mut key_code: Option<Code> = None;

    for raw in combo.split('+') {
        let p = raw.trim();
        if p.is_empty() {
            continue;
        }
        match p {
            "Ctrl" => mods |= Modifiers::CONTROL,
            "Alt" => mods |= Modifiers::ALT,
            "Shift" => mods |= Modifiers::SHIFT,
            "Space" => key_code = Some(Code::Space),
            "ArrowRight" | "Right" => key_code = Some(Code::ArrowRight),
            "ArrowLeft" | "Left" => key_code = Some(Code::ArrowLeft),
            "ArrowUp" | "Up" => key_code = Some(Code::ArrowUp),
            "ArrowDown" | "Down" => key_code = Some(Code::ArrowDown),
            "Enter" => key_code = Some(Code::Enter),
            "Tab" => key_code = Some(Code::Tab),
            "Backspace" => key_code = Some(Code::Backspace),
            "Insert" => key_code = Some(Code::Insert),
            "Delete" => key_code = Some(Code::Delete),
            "Home" => key_code = Some(Code::Home),
            "End" => key_code = Some(Code::End),
            "PageUp" => key_code = Some(Code::PageUp),
            "PageDown" => key_code = Some(Code::PageDown),
            "Minus" => key_code = Some(Code::Minus),
            "Equals" | "Plus" => key_code = Some(Code::Equal),
            "Digit0" | "0" => key_code = Some(Code::Digit0),
            "Digit1" | "1" => key_code = Some(Code::Digit1),
            "Digit2" | "2" => key_code = Some(Code::Digit2),
            "Digit3" | "3" => key_code = Some(Code::Digit3),
            "Digit4" | "4" => key_code = Some(Code::Digit4),
            "Digit5" | "5" => key_code = Some(Code::Digit5),
            "Digit6" | "6" => key_code = Some(Code::Digit6),
            "Digit7" | "7" => key_code = Some(Code::Digit7),
            "Digit8" | "8" => key_code = Some(Code::Digit8),
            "Digit9" | "9" => key_code = Some(Code::Digit9),
            "Numpad0" | "Num0" => key_code = Some(Code::Numpad0),
            "Numpad1" | "Num1" => key_code = Some(Code::Numpad1),
            "Numpad2" | "Num2" => key_code = Some(Code::Numpad2),
            "Numpad3" | "Num3" => key_code = Some(Code::Numpad3),
            "Numpad4" | "Num4" => key_code = Some(Code::Numpad4),
            "Numpad5" | "Num5" => key_code = Some(Code::Numpad5),
            "Numpad6" | "Num6" => key_code = Some(Code::Numpad6),
            "Numpad7" | "Num7" => key_code = Some(Code::Numpad7),
            "Numpad8" | "Num8" => key_code = Some(Code::Numpad8),
            "Numpad9" | "Num9" => key_code = Some(Code::Numpad9),
            "F1" => key_code = Some(Code::F1),
            "F2" => key_code = Some(Code::F2),
            "F3" => key_code = Some(Code::F3),
            "F4" => key_code = Some(Code::F4),
            "F5" => key_code = Some(Code::F5),
            "F6" => key_code = Some(Code::F6),
            "F7" => key_code = Some(Code::F7),
            "F8" => key_code = Some(Code::F8),
            "F9" => key_code = Some(Code::F9),
            "F10" => key_code = Some(Code::F10),
            "F11" => key_code = Some(Code::F11),
            "F12" => key_code = Some(Code::F12),
            "A" => key_code = Some(Code::KeyA),
            "B" => key_code = Some(Code::KeyB),
            "C" => key_code = Some(Code::KeyC),
            "D" => key_code = Some(Code::KeyD),
            "E" => key_code = Some(Code::KeyE),
            "F" => key_code = Some(Code::KeyF),
            "G" => key_code = Some(Code::KeyG),
            "H" => key_code = Some(Code::KeyH),
            "I" => key_code = Some(Code::KeyI),
            "J" => key_code = Some(Code::KeyJ),
            "K" => key_code = Some(Code::KeyK),
            "L" => key_code = Some(Code::KeyL),
            "M" => key_code = Some(Code::KeyM),
            "N" => key_code = Some(Code::KeyN),
            "O" => key_code = Some(Code::KeyO),
            "P" => key_code = Some(Code::KeyP),
            "Q" => key_code = Some(Code::KeyQ),
            "R" => key_code = Some(Code::KeyR),
            "S" => key_code = Some(Code::KeyS),
            "T" => key_code = Some(Code::KeyT),
            "U" => key_code = Some(Code::KeyU),
            "V" => key_code = Some(Code::KeyV),
            "W" => key_code = Some(Code::KeyW),
            "X" => key_code = Some(Code::KeyX),
            "Y" => key_code = Some(Code::KeyY),
            "Z" => key_code = Some(Code::KeyZ),
            _ => {}
        }
    }

    let code = key_code?;
    let m_opt = if mods.is_empty() { None } else { Some(mods) };
    Some(HotKey::new(m_opt, code))
}
#[derive(PartialEq)]
enum AppScreen {
    Onboarding,
    ReadyTransition(Instant),
    MainPlayer,
}

#[derive(PartialEq, Clone, Copy)]
enum LeftTab {
    Playlists,
    LocalFiles,
    Settings,
}

struct OstinatoApp {
    config: AppConfig,
    screen: AppScreen,
    active_tab: LeftTab,

    auth_rx: Option<Receiver<AuthMsg>>,
    device_code_text: Option<String>,
    device_verify_url: Option<String>,
    auth_status: String,
    manual_mode: bool,

    api_tx: Sender<ApiMsg>,
    api_rx: Receiver<ApiMsg>,
    ym_uid: Option<u64>,
    liked_ids: Vec<String>,
    sync_status: String,

    audio_tx: Sender<AudioCmd>,
    audio_state: Arc<SharedAudioState>,
    play_seq: Arc<AtomicU64>,

    hotkey_mgr: Option<GlobalHotKeyManager>,
    registered_hotkeys: Vec<HotKey>,
    hotkey_map: HashMap<u32, BindAction>,
    hk_rx: Receiver<u32>,
    _tray_icon: Option<TrayIcon>,
    tray_wake_rx: Receiver<()>,
    wave_loading: bool,

    token_input: String,
    show_token: bool,
    rebinding_action: Option<String>,

    is_playing: bool,
    is_super_sleep: bool,
    current_pos: f32,
    current_track: Option<TrackItem>,
    current_queue_idx: Option<usize>,

    playlists: Vec<PlaylistEntry>,
    selected_playlist: usize,
    tracks: Vec<TrackItem>,
    local_tracks: Vec<TrackItem>,
    search_query: String,
    search_artists: Vec<ArtistItem>,
    view_title: Option<String>,
    saved_playlist_tracks: Vec<TrackItem>,

}

impl OstinatoApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_ostinato_theme(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let mut config: AppConfig = fs::read_to_string(CONFIG_FILE)
            .ok()
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or_else(|| AppConfig {
                volume: 0.7,
                ..Default::default()
            });

        if config.volume == 0.0 && config.token.is_empty() {
            config.volume = 0.7;
        }
        config.volume = config.volume.clamp(0.0, 1.0);

        let has_token = !config.token.trim().is_empty();
        let screen = if has_token {
            AppScreen::MainPlayer
        } else {
            AppScreen::Onboarding
        };

        let token_input = config.token.clone();
        let (api_tx, api_rx) = mpsc::channel();
        let (audio_tx, audio_state) = spawn_audio_engine();
        let hotkey_mgr = GlobalHotKeyManager::new().ok();

         let (hk_tx, hk_rx) = mpsc::channel::<u32>();
        let ctx_wakeup = cc.egui_ctx.clone();
        let bg_hotkey_map = Arc::new(Mutex::new(HashMap::<u32, BindAction>::new()));
        let bg_hk_ref = Arc::clone(&bg_hotkey_map);
        let bg_audio_tx = audio_tx.clone();
        let bg_audio_st = Arc::clone(&audio_state);

        thread::spawn(move || {
            let receiver = GlobalHotKeyEvent::receiver();
            while let Ok(ev) = receiver.recv() {
                if ev.state == HotKeyState::Pressed {
                    let act_opt = bg_hk_ref.lock().ok().and_then(|m| m.get(&ev.id).copied());
                    if let Some(act) = act_opt {
                        match act {
                            BindAction::PlayPause => {
                                let was_playing = bg_audio_st.playing.load(Ordering::Relaxed);
                                if was_playing {
                                    let _ = bg_audio_tx.send(AudioCmd::Pause);
                                } else {
                                    let _ = bg_audio_tx.send(AudioCmd::Play);
                                }
                            }
                            BindAction::SuperSleep => {
                                force_wake_window_win32();
                            }
                            _ => {}
                        }
                    }
                    let _ = hk_tx.send(ev.id);
                    ctx_wakeup.request_repaint();
                }
            }
        });


        let (tray_tx, tray_wake_rx) = mpsc::channel::<()>();
        let (_tray_icon, show_id, quit_id) = build_ostinato_tray();

        let tray_tx_menu = tray_tx.clone();
        let ctx_menu = cc.egui_ctx.clone();
        thread::spawn(move || {
            let rx = MenuEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if ev.id == quit_id {
                    std::process::exit(0);
                } else if ev.id == show_id {
                    force_wake_window_win32();
                    let _ = tray_tx_menu.send(());
                    ctx_menu.request_repaint();
                }
            }
        });

        let ctx_tray = cc.egui_ctx.clone();
        thread::spawn(move || {
            let rx = TrayIconEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = ev
                {
                    force_wake_window_win32();
                    let _ = tray_tx.send(());
                    ctx_tray.request_repaint();
                }
            }
        });

        let st_watch = Arc::clone(&audio_state);
        let ctx_audio = cc.egui_ctx.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(450));
            if st_watch.finished.load(Ordering::Relaxed) {
                ctx_audio.request_repaint();
            }
        });

        let mut app = Self {
            search_query: String::new(),
            search_artists: Vec::new(),
            view_title: None,
            saved_playlist_tracks: Vec::new(),
            config,
            screen,
            active_tab: LeftTab::Playlists,
            auth_rx: None,
            device_code_text: None,
            device_verify_url: None,
            auth_status: String::new(),
            manual_mode: false,
            api_tx,
            api_rx,
            ym_uid: None,
            liked_ids: Vec::new(),
            sync_status: String::new(),
            audio_tx,
            audio_state,
            play_seq: Arc::new(AtomicU64::new(0)),
            hotkey_mgr,
            registered_hotkeys: Vec::new(),
            hotkey_map: HashMap::new(),
            hk_rx,
            _tray_icon,
            tray_wake_rx,
            wave_loading: false,
            token_input,
            show_token: false,
            rebinding_action: None,
            is_playing: false,
            is_super_sleep: false,
            current_pos: 0.0,
            current_track: None,
            current_queue_idx: None,
            playlists: vec![
                PlaylistEntry {
                    title: "Моя волна".into(),
                    source: PlaylistSource::Wave,
                },
                PlaylistEntry {
                    title: "Мне нравится".into(),
                    source: PlaylistSource::Likes,
                },
            ],
            selected_playlist: 1,
            tracks: Vec::new(),
            local_tracks: Vec::new(),
        };

        app.scan_local_folder();
        app.reload_global_hotkeys();
        if has_token {
            app.trigger_ym_init(cc.egui_ctx.clone());
        }
        app
    }
    fn reload_global_hotkeys(&mut self) {
        let Some(mgr) = &self.hotkey_mgr else { return };
        for hk in self.registered_hotkeys.drain(..) {
            let _ = mgr.unregister(hk);
        }
        self.hotkey_map.clear();

        let pairs = [
            (&self.config.keybinds.play_pause, BindAction::PlayPause),
            (&self.config.keybinds.next_track, BindAction::Next),
            (&self.config.keybinds.prev_track, BindAction::Prev),
            (&self.config.keybinds.like_track, BindAction::Like),
            (&self.config.keybinds.shake_wave, BindAction::ShakeWave),
            (&self.config.keybinds.super_sleep, BindAction::SuperSleep),
        ];

        for (combo, act) in pairs {
            if let Some(hk) = parse_hotkey_str(combo) {
                if mgr.register(hk).is_ok() {
                    self.hotkey_map.insert(hk.id(), act);
                    self.registered_hotkeys.push(hk);
                }
            }
        }
    }

    fn trigger_ym_init(&mut self, ctx: egui::Context) {
        let tok = self.config.token.trim().to_string();
        if tok.is_empty() {
            return;
        }
        self.sync_status = "Синхронизация...".into();
        let tx = self.api_tx.clone();
        thread::spawn(move || {
            match fetch_ym_init(&tok) {
                Ok((uid, playlists, liked_ids, initial_tracks)) => {
                    let _ = tx.send(ApiMsg::InitDone {
                        uid,
                        playlists,
                        liked_ids,
                        initial_tracks,
                    });
                }
                Err(e) => {
                    let _ = tx.send(ApiMsg::Error(e));
                }
            }
            ctx.request_repaint();
        });
    }

    fn trigger_search(&mut self, ctx: &egui::Context) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() {
            self.exit_special_view();
            return;
        }
        let tok = self.config.token.clone();
        if tok.is_empty() {
            return;
        }
        if self.view_title.is_none() {
            self.saved_playlist_tracks = self.tracks.clone();
        }
        self.view_title = Some(format!("Поиск: {}", q));
        self.sync_status = "Поиск...".into();

        let liked = self.liked_ids.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        thread::spawn(move || {
            match search_ym_all(&tok, &q, &liked) {
                Ok((artists, tracks)) => {
                    let _ = tx.send(ApiMsg::SearchDone { artists, tracks });
                }
                Err(e) => {
                    let _ = tx.send(ApiMsg::Error(e));
                }
            }
            ctx_c.request_repaint();
        });
    }

    fn open_artist(&mut self, artist_id: String, artist_name: String, ctx: &egui::Context) {
        let tok = self.config.token.clone();
        if tok.is_empty() {
            return;
        }
        self.active_tab = LeftTab::Playlists;
        if self.view_title.is_none() {
            self.saved_playlist_tracks = self.tracks.clone();
        }
        self.search_artists.clear();
        self.view_title = Some(format!("Артист: {}", artist_name));
        self.sync_status = format!("Загрузка {}...", artist_name);

        let liked = self.liked_ids.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        thread::spawn(move || {
            match fetch_artist_tracks(&tok, &artist_id, &liked) {
                Ok(tracks) => {
                    let _ = tx.send(ApiMsg::ArtistLoaded {
                        artist_name,
                        tracks,
                    });
                }
                Err(e) => {
                    let _ = tx.send(ApiMsg::Error(e));
                }
            }
            ctx_c.request_repaint();
        });
    }

    fn exit_special_view(&mut self) {
        if self.view_title.is_some() {
            self.view_title = None;
            self.search_artists.clear();
            self.search_query.clear();
            if !self.saved_playlist_tracks.is_empty() {
                self.tracks = std::mem::take(&mut self.saved_playlist_tracks);
            }
        }
    }

    fn select_playlist(&mut self, idx: usize, ctx: &egui::Context) {
        self.view_title = None;
        self.search_artists.clear();
        if idx >= self.playlists.len() {
            return;
        }
        self.selected_playlist = idx;
        let Some(uid) = self.ym_uid else { return };
        let tok = self.config.token.clone();
        let src = self.playlists[idx].source.clone();
        let liked = self.liked_ids.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        self.sync_status = "Загрузка плейлиста...".into();

        thread::spawn(move || {
            match fetch_playlist_tracks(&tok, uid, &src, &liked) {
                Ok(list) => {
                    let _ = tx.send(ApiMsg::TracksLoaded(list));
                }
                Err(e) => {
                    let _ = tx.send(ApiMsg::Error(e));
                }
            }
            ctx_c.request_repaint();
        });
    }


    fn start_track(&mut self, track: TrackItem, queue_idx: Option<usize>, ctx: &egui::Context) {
        let seq = self.play_seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.current_queue_idx = queue_idx;
        self.current_pos = 0.0;
        self.current_track = Some(track.clone());

        if let Some(path) = track.local_path {
            self.sync_status.clear();
            self.is_playing = true;
            let _ = self.audio_tx.send(AudioCmd::LoadFile(path, self.config.volume));
        } else {
            let tok = self.config.token.clone();
            if tok.is_empty() {
                return;
            }
            self.sync_status = "Буферизация...".into();
            let tx = self.api_tx.clone();
            let seq_ref = Arc::clone(&self.play_seq);
            let ctx_c = ctx.clone();

            thread::spawn(move || {
                match download_track_mp3(&tok, &track.id) {
                    Ok(bytes) => {
                        if seq_ref.load(Ordering::SeqCst) == seq {
                            let _ = tx.send(ApiMsg::AudioReady {
                                seq,
                                track,
                                bytes: Arc::new(bytes),
                            });
                        }
                    }
                    Err(e) => {
                        if seq_ref.load(Ordering::SeqCst) == seq {
                            let _ = tx.send(ApiMsg::Error(e));
                        }
                    }
                }
                ctx_c.request_repaint();
            });
        }
    }

    fn toggle_play_pause(&mut self, ctx: &egui::Context) {
        if self.current_track.is_none() {
            let active_list = if self.active_tab == LeftTab::LocalFiles {
                &self.local_tracks
            } else {
                &self.tracks
            };
            if let Some(first) = active_list.first().cloned() {
                self.start_track(first, Some(0), ctx);
            }
            return;
        }
        self.is_playing = !self.is_playing;
        let cmd = if self.is_playing {
            AudioCmd::Play
        } else {
            AudioCmd::Pause
        };
        let _ = self.audio_tx.send(cmd);
    }
    fn is_wave_active(&self) -> bool {
        self.active_tab == LeftTab::Playlists
            && self
                .playlists
                .get(self.selected_playlist)
                .map(|p| p.source == PlaylistSource::Wave)
                .unwrap_or(false)
    }

    fn request_more_wave(&mut self, auto_play_next: bool, ctx: &egui::Context) {
        if self.wave_loading {
            return;
        }
        self.wave_loading = true;
        let tok = self.config.token.clone();
        let liked = self.liked_ids.clone();
        let last_id = self.tracks.last().map(|t| t.id.clone());
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        self.sync_status = "Подгрузка волны...".into();

        thread::spawn(move || {
            match fetch_wave_batch(&tok, &liked, last_id.as_deref()) {
                Ok(more) => {
                    let _ = tx.send(ApiMsg::WaveAppended {
                        tracks: more,
                        auto_play_next,
                    });
                }
                Err(e) => {
                    let _ = tx.send(ApiMsg::Error(e));
                }
            }
            ctx_c.request_repaint();
        });
    }

    fn step_queue(&mut self, delta: isize, ctx: &egui::Context) {
        let is_local = self.active_tab == LeftTab::LocalFiles && !self.local_tracks.is_empty();
        let len = if is_local {
            self.local_tracks.len()
        } else {
            self.tracks.len()
        };
        if len == 0 {
            return;
        }

        let cur = self.current_queue_idx.unwrap_or(0) as isize;
        let raw_next = cur + delta;

        if !is_local && self.is_wave_active() && delta > 0 {
            if raw_next >= len as isize {
                self.request_more_wave(true, ctx);
                return;
            } else if (raw_next as usize) + 2 >= len {
                self.request_more_wave(false, ctx);
            }
        }

        let next_idx = ((raw_next % len as isize) + len as isize) as usize % len;
        let tr = if is_local {
            self.local_tracks[next_idx].clone()
        } else {
            self.tracks[next_idx].clone()
        };
        self.start_track(tr, Some(next_idx), ctx);
    }

    fn prev_or_restart(&mut self, ctx: &egui::Context) {
        if self.current_pos > 3.0 {
            self.current_pos = 0.0;
            let _ = self.audio_tx.send(AudioCmd::Seek(0.0));
        } else {
            self.step_queue(-1, ctx);
        }
    }

    fn toggle_current_like(&mut self) {
        let Some(tr) = &mut self.current_track else { return };
        if tr.local_path.is_some() {
            return;
        }
        tr.liked = !tr.liked;
        let new_state = tr.liked;
        let tid = tr.id.clone();

        if new_state {
            if !self.liked_ids.contains(&tid) {
                self.liked_ids.insert(0, tid.clone());
            }
        } else {
            self.liked_ids.retain(|x| x != &tid);
        }
        for item in &mut self.tracks {
            if item.id == tid {
                item.liked = new_state;
            }
        }

        if let Some(uid) = self.ym_uid {
            let tok = self.config.token.clone();
            thread::spawn(move || {
                let client = shared_ym_client();
                let action = if new_state { "add-multiple" } else { "remove" };
                let url = format!("{}/users/{}/likes/tracks/{}", ym_api_base(), uid, action);
                let _ = client
                    .post(&url)
                    .header("Authorization", format!("OAuth {}", tok))
                    .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
                    .form(&[("track-ids", tid.as_str())])
                    .send();
            });
        }
    }
    fn shake_my_wave(&mut self, ctx: &egui::Context) {
        self.active_tab = LeftTab::Playlists;
        self.selected_playlist = 0;
        let tok = self.config.token.clone();
        if tok.is_empty() {
            return;
        }
        let seq = self.play_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let seq_ref = Arc::clone(&self.play_seq);
        let liked = self.liked_ids.clone();
        let last_id = self.current_track.as_ref().map(|t| t.id.clone());
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        self.sync_status = "Встряхиваем Мою волну...".into();

        thread::spawn(move || {
            match fetch_wave_batch(&tok, &liked, last_id.as_deref()) {
                Ok(mut list) => {
                    if let Some(first) = list.first().cloned() {
                        if let Ok(bytes) = download_track_mp3(&tok, &first.id) {
                            if seq_ref.load(Ordering::SeqCst) == seq {
                                let _ = tx.send(ApiMsg::AudioReady {
                                    seq,
                                    track: first,
                                    bytes: Arc::new(bytes),
                                });
                            }
                        }
                    }
                    let _ = tx.send(ApiMsg::TracksLoaded(std::mem::take(&mut list)));
                }
                Err(e) => {
                    let _ = tx.send(ApiMsg::Error(e));
                }
            }
            ctx_c.request_repaint();
        });
    }

    fn start_device_auth(&mut self, ctx: egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.auth_rx = Some(rx);
        self.auth_status = "Запрос кода авторизации...".into();
        self.device_code_text = None;

        thread::spawn(move || {
            let cid = format!("{}{}", CLIENT_ID_A, CLIENT_ID_B);
            let csec = format!("{}{}", SECRET_A, SECRET_B);

            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(987654321);
            let dev_id = format!("os{:08x}", (nanos & 0xFFFF_FFFF) as u32);

            let client = shared_ym_client();
            let code_ep = format!("https://{}.{}/device/code", "oauth", "yandex.ru");
            let code_params = [
                ("client_id", cid.as_str()),
                ("device_id", dev_id.as_str()),
                ("device_name", "YandexMusicAPI"),
            ];

            let resp = client
                .post(&code_ep)
                .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
                .header("Accept", "application/json")
                .form(&code_params)
                .send();

            let raw_text = match resp.and_then(|r| r.text()) {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.send(AuthMsg::Error(format!("Ошибка сети: {}", e)));
                    ctx.request_repaint();
                    return;
                }
            };

            let parsed: DeviceCodeResp = match serde_json::from_str(&raw_text) {
                Ok(d) => d,
                Err(_) => {
                    let _ = tx.send(AuthMsg::Error("Сбой ответа сервера кода".into()));
                    ctx.request_repaint();
                    return;
                }
            };

            if let Some(err) = parsed.error {
                let desc = parsed.error_description.unwrap_or_default();
                let _ = tx.send(AuthMsg::Error(format!("Отказ: {} ({})", err, desc)));
                ctx.request_repaint();
                return;
            }

            let dev_code = parsed.device_code.unwrap_or_default();
            let usr_code = parsed.user_code.unwrap_or_default();
            let ver_url = parsed
                .verification_url
                .unwrap_or_else(|| format!("https://{}/{}", "ya.ru", "device"));

            let _ = open::that(&ver_url);
            let _ = tx.send(AuthMsg::CodeReady {
                code: usr_code,
                url: ver_url,
            });
            ctx.request_repaint();

            let tok_ep = format!("https://{}.{}/token", "oauth", "yandex.ru");
            let wait = Duration::from_secs(parsed.interval.unwrap_or(5).max(3));
            for _ in 1..=60 {
                thread::sleep(wait);
                let poll = [
                    ("grant_type", "device_code"),
                    ("code", dev_code.as_str()),
                    ("client_id", cid.as_str()),
                    ("client_secret", csec.as_str()),
                ];

                if let Ok(r) = client
                    .post(&tok_ep)
                    .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
                    .header("Accept", "application/json")
                    .form(&poll)
                    .send()
                {
                    let body = r.text().unwrap_or_default();
                    if body.contains("authorization_pending") {
                        continue;
                    }
                    if body.contains("slow_down") {
                        thread::sleep(Duration::from_secs(3));
                        continue;
                    }
                    if !body.trim_start().starts_with('{') {
                        continue;
                    }

                    if let Ok(tok) = serde_json::from_str::<TokenPollResp>(&body) {
                        if let Some(access) = tok.access_token {
                            let _ = tx.send(AuthMsg::Success(access));
                            ctx.request_repaint();
                            return;
                        }
                        if let Some(err) = tok.error {
                            let _ = tx.send(AuthMsg::Error(format!("Отказ OAuth: {}", err)));
                            ctx.request_repaint();
                            return;
                        }
                    }
                }
            }
            let _ = tx.send(AuthMsg::Error("Время ожидания истекло.".into()));
            ctx.request_repaint();
        });
    }
    fn save_config(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.config) {
            let _ = fs::write(CONFIG_FILE, json);
        }
    }

    fn scan_local_folder(&mut self) {
        self.local_tracks.clear();
        if let Some(folder) = &self.config.local_folder {
            if let Ok(entries) = fs::read_dir(folder) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if matches!(ext.to_lowercase().as_str(), "mp3" | "flac" | "wav" | "ogg") {
                            let name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("Track")
                                .to_string();
                            self.local_tracks.push(TrackItem {
                                id: name.clone(),
                                title: name,
                                artist: "Локальный файл".into(),
                                duration_sec: 200.0,
                                liked: false,
                                cover_url: None,
                                artist_id: None,
                                local_path: Some(path),
                            });
                        }
                    }
                }
            }
        }
    }

    fn wake_from_super_sleep(&mut self, ctx: &egui::Context) {
        self.is_super_sleep = false;
        set_process_eco_qos(false);
        force_wake_window_win32();
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(180.0, 140.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn trigger_super_sleep(&mut self, ctx: &egui::Context) {
        capture_own_hwnd();
        if self.is_super_sleep {
            self.wake_from_super_sleep(ctx);
        } else {
            force_hide_window_win32();
            self.is_super_sleep = true;
            set_process_eco_qos(true);

            thread::spawn(|| {
                thread::sleep(Duration::from_millis(120));
                force_hide_window_win32();
                deep_trim_memory();
                thread::sleep(Duration::from_millis(500));
                deep_trim_memory();
            });
        }
    }
}

fn setup_ostinato_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(TEXT_BLUE);
    visuals.panel_fill = BG_MAIN;
    visuals.window_fill = BG_PANEL;
    visuals.extreme_bg_color = BG_VOID;
    visuals.faint_bg_color = BG_WIDGET;
    visuals.code_bg_color = BG_VOID;
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER_DARK_BLUE);
    visuals.resize_corner_size = 0.0;

    visuals.selection.bg_fill = BLUE_DEEP;
    visuals.selection.stroke = Stroke::new(1.0_f32, BLUE_NEON);

    visuals.widgets.noninteractive.bg_fill = BG_VOID;
    visuals.widgets.noninteractive.weak_bg_fill = BG_VOID;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER_DARK_BLUE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_BLUE);

    // Неактивный ползунок скролла — темно-синий
    visuals.widgets.inactive.bg_fill = BLUE_DEEP;
    visuals.widgets.inactive.weak_bg_fill = BG_WIDGET;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_DARK_BLUE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_BLUE);
    visuals.widgets.inactive.rounding = Rounding::same(6.0_f32);

    // Ползунок скролла при наведении — неоново-синий вместо серо-белого!
    visuals.widgets.hovered.bg_fill = BLUE_NEON;
    visuals.widgets.hovered.weak_bg_fill = BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BLUE_NEON);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, BLUE_BRIGHT);
    visuals.widgets.hovered.rounding = Rounding::same(6.0_f32);

    visuals.widgets.active.bg_fill = BLUE_DEEP;
    visuals.widgets.active.weak_bg_fill = BLUE_DEEP;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, BLUE_BRIGHT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, BLUE_BRIGHT);
    visuals.widgets.active.rounding = Rounding::same(6.0_f32);

    visuals.widgets.open.bg_fill = BG_VOID;
    visuals.widgets.open.weak_bg_fill = BG_VOID;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, BLUE_NEON);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, BLUE_BRIGHT);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.window_margin = egui::Margin::ZERO;
    // Тонкий аккуратный плавающий скроллбар 4 пикселя
    style.spacing.scroll.floating = true;
    style.spacing.scroll.bar_width = 4.0;
    style.spacing.scroll.floating_width = 4.0;
    style.spacing.scroll.bar_inner_margin = 2.0;
    style.spacing.scroll.bar_outer_margin = 2.0;
    style.spacing.scroll.foreground_color = false; // Выключает светлую заливку ползунка!
    ctx.set_style(style);
}


impl OstinatoApp {
    fn handle_window_resize(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let rect = ctx.screen_rect();
        let border = 6.0;
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };

        let left = pos.x <= rect.left() + border;
        let right = pos.x >= rect.right() - border;
        let top = pos.y <= rect.top() + border;
        let bottom = pos.y >= rect.bottom() - border;

        use egui::{CursorIcon, ResizeDirection};
        let dir = match (left, right, top, bottom) {
            (true, _, true, _) => Some((ResizeDirection::NorthWest, CursorIcon::ResizeNwSe)),
            (_, true, true, _) => Some((ResizeDirection::NorthEast, CursorIcon::ResizeNeSw)),
            (true, _, _, true) => Some((ResizeDirection::SouthWest, CursorIcon::ResizeNeSw)),
            (_, true, _, true) => Some((ResizeDirection::SouthEast, CursorIcon::ResizeNwSe)),
            (true, _, _, _) => Some((ResizeDirection::West, CursorIcon::ResizeHorizontal)),
            (_, true, _, _) => Some((ResizeDirection::East, CursorIcon::ResizeHorizontal)),
            (_, _, true, _) => Some((ResizeDirection::North, CursorIcon::ResizeVertical)),
            (_, _, _, true) => Some((ResizeDirection::South, CursorIcon::ResizeVertical)),
            _ => None,
        };

        if let Some((resize_dir, cursor)) = dir {
            ctx.set_cursor_icon(cursor);
            if ctx.input(|i| i.pointer.primary_pressed()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(resize_dir));
            }
        }
    }

    fn render_custom_titlebar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("ostinato_titlebar")
            .exact_height(32.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::none()
                    .fill(BG_BAR)
                    .inner_margin(egui::Margin::symmetric(12.0, 4.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let drag_width = (ui.available_width() - 110.0).max(50.0);
                    let (drag_rect, drag_resp) = ui.allocate_exact_size(
                        Vec2::new(drag_width, 24.0),
                        Sense::click_and_drag(),
                    );

                    if drag_resp.double_clicked() {
                        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                    } else if drag_resp.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }

                    let status_suffix = if self.sync_status.is_empty() {
                        "LOCAL AUDIO ENGINE".to_string()
                    } else {
                        self.sync_status.clone()
                    };

                    ui.painter().text(
                        drag_rect.left_center(),
                        egui::Align2::LEFT_CENTER,
                        format!("OSTINATO  //  {}", status_suffix),
                        FontId::proportional(11.0),
                        TEXT_GRAY,
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if win_bar_btn(ui, 2).clicked() {
                            std::process::exit(0);
                        }
                        if win_bar_btn(ui, 1).clicked() {
                            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                        }
                        if win_bar_btn(ui, 0).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });
    }
}
impl eframe::App for OstinatoApp {
        fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        BG_VOID.to_normalized_gamma_f32()
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
                if !self.is_super_sleep {
            capture_own_hwnd();
        }

        while self.tray_wake_rx.try_recv().is_ok() {
            self.wake_from_super_sleep(ctx);
        }

        while let Ok(hk_id) = self.hk_rx.try_recv() {
            if let Some(&act) = self.hotkey_map.get(&hk_id) {
                match act {
                    BindAction::PlayPause => self.toggle_play_pause(ctx),
                    BindAction::Next => self.step_queue(1, ctx),
                    BindAction::Prev => self.prev_or_restart(ctx),
                    BindAction::Like => self.toggle_current_like(),
                    BindAction::ShakeWave => self.shake_my_wave(ctx),
                    BindAction::SuperSleep => self.trigger_super_sleep(ctx),
                }
            }
        }

        while let Ok(msg) = self.api_rx.try_recv() {
            match msg {
                ApiMsg::InitDone {
                    uid,
                    playlists,
                    liked_ids,
                    initial_tracks,
                } => {
                    self.ym_uid = Some(uid);
                    self.playlists = playlists;
                    self.liked_ids = liked_ids;
                    self.tracks = initial_tracks;
                    self.sync_status.clear();
                    if self.current_track.is_none() {
                        if let Some(first) = self.tracks.first().cloned() {
                            self.current_track = Some(first);
                            self.current_queue_idx = Some(0);
                        }
                    }
                }
                ApiMsg::TracksLoaded(list) => {
                    self.wave_loading = false;
                    self.tracks = list;
                    self.sync_status.clear();
                }
                ApiMsg::SearchDone { artists, tracks } => {
                    self.search_artists = artists;
                    self.tracks = tracks;
                    self.sync_status.clear();
                }
                ApiMsg::ArtistLoaded { artist_name, tracks } => {
                    self.view_title = Some(format!("Артист: {}", artist_name));
                    self.tracks = tracks;
                    self.sync_status.clear();
                }

                ApiMsg::WaveAppended {
                    tracks: new_tracks,
                    auto_play_next,
                } => {
                    self.wave_loading = false;
                    self.sync_status.clear();
                    let prev_len = self.tracks.len();
                    for t in new_tracks {
                        if !self.tracks.iter().any(|x| x.id == t.id) {
                            self.tracks.push(t);
                        }
                    }
                    if auto_play_next && self.tracks.len() > prev_len {
                        let next_tr = self.tracks[prev_len].clone();
                        self.start_track(next_tr, Some(prev_len), ctx);
                    }
                }
                ApiMsg::AudioReady { seq, track, bytes } => {
                    if seq == self.play_seq.load(Ordering::SeqCst) {
                        self.sync_status.clear();
                        self.current_track = Some(track);
                        self.is_playing = true;
                        let _ = self
                            .audio_tx
                            .send(AudioCmd::LoadBytes(bytes, self.config.volume));
                        if self.is_super_sleep {
                            deep_trim_memory();
                        }
                    }
                }
                ApiMsg::Error(e) => {
                    self.wave_loading = false;
                    self.sync_status = format!("API: {}", e);
                }
            }
        }

        if self.audio_state.finished.swap(false, Ordering::Relaxed) {
            self.step_queue(1, ctx);
        }

        if self.is_super_sleep {
            return;
        }

        if self.is_playing {
            if let Ok(p) = self.audio_state.pos_sec.lock() {
                self.current_pos = *p;
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        let mut clear_auth = false;
        let mut trigger_init = false;
        if let Some(rx) = &self.auth_rx {
            ctx.request_repaint_after(Duration::from_millis(400));
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    AuthMsg::CodeReady { code, url } => {
                        self.device_code_text = Some(code);
                        self.device_verify_url = Some(url);
                        self.auth_status = "Введи код в браузере и подтверди вход...".into();
                    }
                    AuthMsg::Success(token) => {
                        self.config.token = token.clone();
                        self.token_input = token;
                        self.save_config();
                        clear_auth = true;
                        trigger_init = true;
                        self.screen = AppScreen::ReadyTransition(Instant::now());
                    }
                    AuthMsg::Error(err) => {
                        self.auth_status = err;
                        clear_auth = true;
                    }
                }
            }
        }
        if clear_auth {
            self.auth_rx = None;
        }
        if trigger_init {
            self.trigger_ym_init(ctx.clone());
        }

        self.handle_window_resize(ctx);
        self.render_custom_titlebar(ctx);

        match self.screen {
            AppScreen::Onboarding => self.render_onboarding(ctx),
            AppScreen::ReadyTransition(t) => self.render_ready_transition(ctx, t),
            AppScreen::MainPlayer => self.render_main_player(ctx),
        }
    }
}
impl OstinatoApp {
    fn render_onboarding(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_VOID))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(42.0);
                    ui.label(
                        RichText::new("O S T I N A T O")
                            .font(FontId::proportional(32.0))
                            .color(BLUE_NEON)
                            .strong(),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Автономный аудиоклиент // Локальное хранение")
                            .font(FontId::proportional(13.0))
                            .color(TEXT_GRAY),
                    );
                    ui.add_space(22.0);

                    egui::Frame::none()
                        .fill(BG_PANEL)
                        .stroke(Stroke::new(1.0_f32, BORDER_DARK_BLUE))
                        .rounding(Rounding::same(10.0_f32))
                        .inner_margin(24.0)
                        .show(ui, |ui| {
                            ui.set_max_width(450.0);
                            ui.label(
                                RichText::new("Безопасный вход по коду устройства")
                                    .font(FontId::proportional(17.0))
                                    .color(BLUE_BRIGHT)
                                    .strong(),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(
                                    "Вход происходит на сайте Яндекса без ввода пароля в программу. Все данные и токен хранятся ТОЛЬКО локально на вашем ПК в файле ostinato_config.json.",
                                )
                                .font(FontId::proportional(12.0))
                                .color(TEXT_GRAY),
                            );
                            ui.add_space(16.0);

                            if let Some(code) = &self.device_code_text.clone() {
                                egui::Frame::none()
                                    .fill(BG_VOID)
                                    .stroke(Stroke::new(1.5_f32, BLUE_NEON))
                                    .rounding(Rounding::same(8.0_f32))
                                    .inner_margin(12.0)
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("Код подтверждения:")
                                                .size(11.5)
                                                .color(TEXT_GRAY),
                                        );
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(code)
                                                .font(FontId::proportional(28.0))
                                                .color(BLUE_BRIGHT)
                                                .strong(),
                                        );
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            let pad = ((ui.available_width() - 256.0) / 2.0).max(0.0);
                                            ui.add_space(pad);
                                            if pill_action_btn(ui, "Скопировать", false) {
                                                ctx.copy_text(code.clone());
                                            }
                                            if let Some(url) = &self.device_verify_url {
                                                if pill_action_btn(ui, "Открыть сайт", true) {
                                                    let _ = open::that(url);
                                                }
                                            }
                                        });
                                    });
                                ui.add_space(8.0);
                                ui.label(RichText::new(&self.auth_status).size(12.0).color(BLUE_NEON));
                            } else if !self.manual_mode {
                                if full_width_btn(ui, "Получить код и войти", 38.0, true) {
                                    self.start_device_auth(ctx.clone());
                                }
                                if !self.auth_status.is_empty() {
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new(&self.auth_status).size(12.0).color(TEXT_GRAY),
                                    );
                                }
                            } else {
                                ui.add_sized(
                                    [ui.available_width(), 30.0],
                                    egui::TextEdit::singleline(&mut self.token_input)
                                        .text_color(BLUE_BRIGHT)
                                        .password(true)
                                        .hint_text("Токен y0_AgAAAA..."),
                                );
                                ui.add_space(10.0);
                                if !self.token_input.trim().is_empty()
                                    && full_width_btn(ui, "Сохранить токен", 34.0, true)
                                {
                                    self.config.token = extract_token(&self.token_input);
                                    self.save_config();
                                    self.trigger_ym_init(ctx.clone());
                                    self.screen = AppScreen::ReadyTransition(Instant::now());
                                }
                            }

                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                let mode_txt = if self.manual_mode {
                                    "<- Вход по коду"
                                } else {
                                    "Ввести токен вручную"
                                };
                                if pill_action_btn(ui, mode_txt, false) {
                                    self.manual_mode = !self.manual_mode;
                                    self.device_code_text = None;
                                }
                                if pill_action_btn(ui, "Офлайн-папка", false) {
                                    self.active_tab = LeftTab::LocalFiles;
                                    self.screen = AppScreen::ReadyTransition(Instant::now());
                                }
                            });
                        });
                });
            });
    }

    fn render_ready_transition(&mut self, ctx: &egui::Context, start: Instant) {
        let elapsed = start.elapsed().as_secs_f32();
        let duration = 1.8;
        if elapsed >= duration {
            self.screen = AppScreen::MainPlayer;
            return;
        }
        let alpha = if elapsed < 0.45 {
            elapsed / 0.45
        } else if elapsed > 1.35 {
            (duration - elapsed) / 0.45
        } else {
            1.0
        }
        .clamp(0.0, 1.0);

        let main_col = Color32::from_rgba_unmultiplied(55, 165, 255, (alpha * 255.0) as u8);
        let sub_col = Color32::from_rgba_unmultiplied(85, 100, 125, (alpha * 220.0) as u8);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_VOID))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.38);
                    ui.label(
                        RichText::new("Готово!")
                            .font(FontId::proportional(40.0))
                            .color(main_col)
                            .strong(),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Аудиопространство Ostinato активировано")
                            .font(FontId::proportional(14.0))
                            .color(sub_col),
                    );
                });
            });
        ctx.request_repaint();
    }
    fn render_main_player(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("os_left_nav")
            .resizable(false)
            .exact_width(58.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::none()
                    .fill(BG_VOID)
                    .inner_margin(8.0),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("OS")
                            .font(FontId::proportional(18.0))
                            .color(BLUE_NEON)
                            .strong(),
                    );
                    ui.add_space(20.0);

                    if draw_side_nav_btn(
                        ui,
                        "nav_pl",
                        0,
                        self.active_tab == LeftTab::Playlists,
                        "Плейлисты и Волна",
                    ) {
                        self.active_tab = LeftTab::Playlists;
                    }
                    ui.add_space(10.0);

                    if draw_side_nav_btn(
                        ui,
                        "nav_loc",
                        1,
                        self.active_tab == LeftTab::LocalFiles,
                        "Локальная папка",
                    ) {
                        self.active_tab = LeftTab::LocalFiles;
                    }

                    let bottom_space = (ui.available_height() - 46.0).max(10.0);
                    ui.add_space(bottom_space);

                    if draw_side_nav_btn(
                        ui,
                        "nav_cfg",
                        2,
                        self.active_tab == LeftTab::Settings,
                        "Настройки и бинды",
                    ) {
                        self.active_tab = LeftTab::Settings;
                    }
                });
            });

        egui::SidePanel::left("os_sub_panel")
            .resizable(true)
            .default_width(265.0)
            .width_range(215.0..=360.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::none()
                    .fill(BG_PANEL)
                    .inner_margin(14.0),
            )
            .show(ctx, |ui| match self.active_tab {
                LeftTab::Playlists => self.render_sub_playlists(ui, ctx),
                LeftTab::LocalFiles => self.render_sub_local(ui, ctx),
                LeftTab::Settings => self.render_sub_settings(ui, ctx),
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_MAIN).inner_margin(24.0))
            .show(ctx, |ui| {
                self.render_right_player(ui, ctx);
            });
    }

     fn render_sub_playlists(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 34.0).max(80.0);
            let edit_resp = dark_text_input(
                ui,
                w,
                &mut self.search_query,
                "Поиск треков и артистов...",
                false,
            );
            if edit_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.trigger_search(ctx);
            }
            let btn_lbl = if self.view_title.is_some() && !self.search_query.is_empty() {
                "×"
            } else {
                "Go"
            };
            if small_search_btn(ui, btn_lbl) {
                if self.view_title.is_some() && !self.search_query.is_empty() {
                    self.exit_special_view();
                } else {
                    self.trigger_search(ctx);
                }
            }
        });

        ui.add_space(10.0);

        if let Some(title) = self.view_title.clone() {
            if full_width_btn(ui, "<- Назад к плейлистам", 26.0, false) {
                self.exit_special_view();
            }
            ui.add_space(8.0);
            ui.label(RichText::new(title).size(12.0).color(BLUE_NEON).strong());
            ui.add_space(6.0);

            if !self.search_artists.is_empty() {
                ui.label(RichText::new("АРТИСТЫ").size(10.5).color(TEXT_GRAY));
                ui.add_space(4.0);
                let mut open_art: Option<(String, String)> = None;
                for art in &self.search_artists {
                    if artist_card_row(ui, &art.name) {
                        open_art = Some((art.id.clone(), art.name.clone()));
                    }
                    ui.add_space(4.0);
                }
                if let Some((aid, aname)) = open_art {
                    self.open_artist(aid, aname, ctx);
                }
                ui.add_space(8.0);
            }
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("БИБЛИОТЕКА").size(11.5).color(BLUE_NEON).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if small_text_btn(ui, "Обновить") {
                        self.trigger_ym_init(ctx.clone());
                    }
                });
            });
            ui.add_space(6.0);

            let mut clicked_pl = None;
            egui::ScrollArea::vertical()
                .id_salt("pl_scroll")
                .max_height(135.0)
                .show(ui, |ui| {
                    for (idx, pl) in self.playlists.iter().enumerate() {
                        if list_item_row(ui, &pl.title, self.selected_playlist == idx) {
                            clicked_pl = Some(idx);
                        }
                        ui.add_space(2.0);
                    }
                });
            if let Some(idx) = clicked_pl {
                self.select_playlist(idx, ctx);
            }
            ui.add_space(8.0);
        }

        ui.painter().hline(
            ui.available_rect_before_wrap().x_range(),
            ui.cursor().top(),
            Stroke::new(1.0_f32, BORDER_DARK_BLUE),
        );
        ui.add_space(8.0);

        ui.label(
            RichText::new(format!("ТРЕКИ ({})", self.tracks.len()))
                .size(11.0)
                .color(TEXT_GRAY),
        );
        ui.add_space(6.0);

        let mut play_req: Option<(TrackItem, usize)> = None;
        egui::ScrollArea::vertical().id_salt("tr_scroll").show(ui, |ui| {
            for (i, track) in self.tracks.iter().enumerate() {
                let is_curr = self
                    .current_track
                    .as_ref()
                    .map(|t| t.id == track.id)
                    .unwrap_or(false);
                let label = format!("{} — {}", track.artist, track.title);
                if list_item_row(ui, &label, is_curr) {
                    play_req = Some((track.clone(), i));
                }
                ui.add_space(2.0);
            }
        });

        if let Some((tr, idx)) = play_req {
            self.start_track(tr, Some(idx), ctx);
        }
    }


    fn render_sub_local(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(
            RichText::new("ЛОКАЛЬНАЯ ПАПКА")
                .size(11.5)
                .color(BLUE_NEON)
                .strong(),
        );
        ui.add_space(8.0);

        if full_width_btn(ui, "Выбрать папку с треками", 32.0, true) {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                self.config.local_folder = Some(folder);
                self.save_config();
                self.scan_local_folder();
            }
        }
        ui.add_space(10.0);

        if self.local_tracks.is_empty() {
            ui.label(
                RichText::new("Файлы не выбраны (MP3, FLAC, WAV, OGG).")
                    .size(11.5)
                    .color(TEXT_GRAY),
            );
        } else {
            let mut chosen: Option<(TrackItem, usize)> = None;
            egui::ScrollArea::vertical().id_salt("loc_scroll").show(ui, |ui| {
                for (i, track) in self.local_tracks.iter().enumerate() {
                    let is_curr = self
                        .current_track
                        .as_ref()
                        .map(|t| t.local_path == track.local_path)
                        .unwrap_or(false);
                    if list_item_row(ui, &track.title, is_curr) {
                        chosen = Some((track.clone(), i));
                    }
                    ui.add_space(2.0);
                }
            });
            if let Some((tr, idx)) = chosen {
                self.start_track(tr, Some(idx), ctx);
            }
        }
    }

    fn render_sub_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("НАСТРОЙКИ").size(11.5).color(BLUE_NEON).strong());
        ui.add_space(4.0);
        ui.label(
            RichText::new("Все данные хранятся строго локально на вашем ПК.")
                .size(10.5)
                .color(TEXT_GRAY),
        );
        ui.add_space(10.0);

        ui.label(RichText::new("Токен авторизации:").size(11.0).color(TEXT_GRAY));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 36.0).max(60.0);
            if ui
                .add_sized(
                    [width, 26.0],
                    egui::TextEdit::singleline(&mut self.token_input)
                        .text_color(BLUE_BRIGHT)
                        .password(!self.show_token)
                        .hint_text("Пусто"),
                )
                .changed()
            {
                self.config.token = extract_token(&self.token_input);
                self.save_config();
                self.trigger_ym_init(ctx.clone());
            }

            if eye_toggle_btn(ui, self.show_token) {
                self.show_token = !self.show_token;
            }
        });

        ui.add_space(6.0);
        if full_width_btn(ui, "Привязать аккаунт по коду заново", 26.0, false) {
            self.device_code_text = None;
            self.manual_mode = false;
            self.screen = AppScreen::Onboarding;
        }

        ui.add_space(12.0);
        ui.painter().hline(
            ui.available_rect_before_wrap().x_range(),
            ui.cursor().top(),
            Stroke::new(1.0_f32, BORDER_DARK_BLUE),
        );
        ui.add_space(10.0);

        ui.label(
            RichText::new("ГОРЯЧИЕ КЛАВИШИ (ПОВЕРХ ИГР)")
                .size(11.0)
                .color(BLUE_NEON)
                .strong(),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new("Возврат из Супер-сна также доступен через значок у часов")
                .size(10.0)
                .color(TEXT_GRAY),
        );
        ui.add_space(6.0);

        if let Some(action_id) = self.rebinding_action.clone() {
            match capture_key_combo(ctx) {
                KeyCapture::Combo(combo) => {
                    match action_id.as_str() {
                        "play_pause" => self.config.keybinds.play_pause = combo,
                        "next" => self.config.keybinds.next_track = combo,
                        "prev" => self.config.keybinds.prev_track = combo,
                        "like" => self.config.keybinds.like_track = combo,
                        "wave" => self.config.keybinds.shake_wave = combo,
                        "sleep" => self.config.keybinds.super_sleep = combo,
                        _ => {}
                    }
                    self.rebinding_action = None;
                    self.save_config();
                    self.reload_global_hotkeys();
                }
                KeyCapture::Cancelled => {
                    self.rebinding_action = None;
                }
                KeyCapture::None => {}
            }
        }

        egui::ScrollArea::vertical().id_salt("binds_scroll").show(ui, |ui| {
            Self::bind_row(
                ui,
                "Пауза / Воспроизведение",
                "play_pause",
                &self.config.keybinds.play_pause.clone(),
                &mut self.rebinding_action,
            );
            Self::bind_row(
                ui,
                "Следующий трек",
                "next",
                &self.config.keybinds.next_track.clone(),
                &mut self.rebinding_action,
            );
            Self::bind_row(
                ui,
                "Предыдущий / В начало",
                "prev",
                &self.config.keybinds.prev_track.clone(),
                &mut self.rebinding_action,
            );
            Self::bind_row(
                ui,
                "Мне нравится (Лайк)",
                "like",
                &self.config.keybinds.like_track.clone(),
                &mut self.rebinding_action,
            );
            Self::bind_row(
                ui,
                "Встряхнуть Мою волну",
                "wave",
                &self.config.keybinds.shake_wave.clone(),
                &mut self.rebinding_action,
            );
            Self::bind_row(
                ui,
                "Супер-сон (Скрыть в трей)",
                "sleep",
                &self.config.keybinds.super_sleep.clone(),
                &mut self.rebinding_action,
            );
        });
    }

    fn bind_row(
        ui: &mut egui::Ui,
        label: &str,
        id: &str,
        current_val: &str,
        rebinding: &mut Option<String>,
    ) {
        ui.label(RichText::new(label).size(11.0).color(TEXT_GRAY));
        let is_waiting = rebinding.as_deref() == Some(id);
        let txt = if is_waiting {
            "Нажмите клавишу (Esc — отмена)..."
        } else {
            current_val
        };
        if full_width_btn(ui, txt, 26.0, is_waiting) {
            *rebinding = if is_waiting { None } else { Some(id.to_string()) };
        }
        ui.add_space(5.0);
    }
    fn render_right_player(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if pill_action_btn(ui, "Встряхнуть волну", false) {
                self.shake_my_wave(ctx);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill_action_btn(ui, "СУПЕР-СОН", true) {
                    self.trigger_super_sleep(ctx);
                }
            });
        });

        ui.add_space(26.0);

        ui.vertical_centered(|ui| {
            let cover_size = Vec2::new(210.0, 210.0);
            let has_cover = self
                .current_track
                .as_ref()
                .and_then(|t| t.cover_url.clone());

            if let Some(url) = has_cover {
                let img = egui::Image::from_uri(url)
                    .fit_to_exact_size(cover_size)
                    .rounding(Rounding::same(16.0_f32));
                let resp = ui.add(img);
                ui.painter().rect_stroke(
                    resp.rect,
                    Rounding::same(16.0_f32),
                    Stroke::new(1.5_f32, BORDER_DARK_BLUE),
                );
            } else {
                let (rect, _) = ui.allocate_exact_size(cover_size, Sense::hover());
                ui.painter().rect(
                    rect,
                    Rounding::same(16.0_f32),
                    BG_VOID,
                    Stroke::new(1.5_f32, BORDER_DARK_BLUE),
                );
                let c = rect.center();
                let anim = if self.is_playing {
                    (ctx.input(|i| i.time) as f32 * 4.0).sin().abs() * 8.0
                } else {
                    0.0
                };
                let heights = [16.0 + anim, 34.0 - anim * 0.5, 24.0 + anim * 0.7, 14.0];
                for (x_off, h) in [-16.0_f32, -5.0, 6.0, 17.0].iter().zip(heights) {
                    let bar = egui::Rect::from_center_size(egui::pos2(c.x + x_off, c.y), Vec2::new(4.5, h));
                    ui.painter().rect_filled(bar, Rounding::same(2.0_f32), BLUE_DEEP);
                }
            }


            let (title, artist, artist_id, duration, liked) = if let Some(t) = &self.current_track {
                (
                    t.title.clone(),
                    t.artist.clone(),
                    t.artist_id.clone(),
                    t.duration_sec.max(1.0),
                    t.liked,
                )
            } else {
                ("Выберите трек".into(), "Ostinato".into(), None, 100.0, false)
            };

            let short_title: String = title.chars().take(42).collect();
            let short_artist: String = artist.chars().take(48).collect();
            ui.label(
                RichText::new(short_title)
                    .font(FontId::proportional(21.0))
                    .color(BLUE_BRIGHT)
                    .strong(),
            );
            ui.add_space(4.0);

            if let Some(aid) = artist_id {
                let text_w = (short_artist.chars().count() as f32 * 7.6 + 24.0).clamp(70.0, 340.0);
                let (art_rect, art_resp) =
                    ui.allocate_exact_size(Vec2::new(text_w, 24.0), Sense::click());
                let hov = art_resp.hovered();
                if hov {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    ui.painter().rect(
                        art_rect,
                        Rounding::same(12.0_f32),
                        BG_HOVER,
                        Stroke::new(1.0_f32, BLUE_NEON),
                    );
                }
                let art_col = if hov { BLUE_BRIGHT } else { TEXT_GRAY };
                let disp = if hov {
                    format!("{}  ->", short_artist)
                } else {
                    short_artist
                };
                ui.painter().text(
                    art_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    disp,
                    FontId::proportional(13.5),
                    art_col,
                );
                if art_resp.clicked() {
                    self.open_artist(aid, artist.clone(), ctx);
                }
            } else {
                ui.label(
                    RichText::new(short_artist)
                        .font(FontId::proportional(13.5))
                        .color(TEXT_GRAY),
                );
            }


            ui.add_space(26.0);


            let bar_w = (ui.available_width() - 120.0).clamp(180.0, 460.0);
            let total_row_w = bar_w + 88.0;
            let (_, row_rect) = ui.allocate_space(Vec2::new(total_row_w, 20.0));

            let left_t = egui::Rect::from_min_size(row_rect.min, Vec2::new(38.0, 20.0));
            let right_t = egui::Rect::from_min_size(
                egui::pos2(row_rect.max.x - 38.0, row_rect.min.y),
                Vec2::new(38.0, 20.0),
            );
            let seek_rect =
                egui::Rect::from_center_size(row_rect.center(), Vec2::new(bar_w, 16.0));

            ui.painter().text(
                left_t.left_center(),
                egui::Align2::LEFT_CENTER,
                format_time(self.current_pos),
                FontId::proportional(11.5),
                TEXT_GRAY,
            );
            ui.painter().text(
                right_t.right_center(),
                egui::Align2::RIGHT_CENTER,
                format_time(duration),
                FontId::proportional(11.5),
                TEXT_GRAY,
            );

            let seek_resp =
                ui.interact(seek_rect, ui.id().with("os_seek_bar"), Sense::click_and_drag());
            if (seek_resp.clicked() || seek_resp.dragged()) && seek_rect.width() > 0.0 {
                if let Some(p) = seek_resp.interact_pointer_pos() {
                    let ratio = ((p.x - seek_rect.left()) / seek_rect.width()).clamp(0.0, 1.0);
                    let target = ratio * duration;
                    self.current_pos = target;
                    let _ = self.audio_tx.send(AudioCmd::Seek(target));
                }
            }

            let is_hov = seek_resp.hovered() || seek_resp.dragged();
            let track_h = if is_hov { 5.5 } else { 3.5 };
            let bg_line =
                egui::Rect::from_center_size(seek_rect.center(), Vec2::new(bar_w, track_h));
            ui.painter()
                .rect_filled(bg_line, Rounding::same(3.0_f32), BG_WIDGET);

            let cur_ratio = (self.current_pos / duration).clamp(0.0, 1.0);
            let cur_x = bg_line.left() + bar_w * cur_ratio;

            if is_hov {
                if let Some(hp) = ctx
                    .input(|i| i.pointer.hover_pos())
                    .or_else(|| seek_resp.interact_pointer_pos())
                {
                    let hov_ratio = ((hp.x - bg_line.left()) / bar_w).clamp(0.0, 1.0);
                    let hov_x = bg_line.left() + bar_w * hov_ratio;
                    let hov_sec = hov_ratio * duration;

                    let preview_rect = egui::Rect::from_min_max(
                        egui::pos2(cur_x.min(hov_x), bg_line.top()),
                        egui::pos2(cur_x.max(hov_x), bg_line.bottom()),
                    );
                    ui.painter().rect_filled(
                        preview_rect,
                        Rounding::same(3.0_f32),
                        Color32::from_rgba_unmultiplied(0, 135, 250, 95),
                    );

                    ui.painter().line_segment(
                        [
                            egui::pos2(hov_x, bg_line.top() - 4.0),
                            egui::pos2(hov_x, bg_line.bottom() + 4.0),
                        ],
                        Stroke::new(1.5_f32, BLUE_BRIGHT),
                    );

                    let badge = egui::Rect::from_center_size(
                        egui::pos2(hov_x, bg_line.top() - 16.0),
                        Vec2::new(44.0, 19.0),
                    );
                    ui.painter().rect(
                        badge,
                        Rounding::same(5.0_f32),
                        BG_BAR,
                        Stroke::new(1.0_f32, BLUE_NEON),
                    );
                    ui.painter().text(
                        badge.center(),
                        egui::Align2::CENTER_CENTER,
                        format_time(hov_sec),
                        FontId::proportional(11.0),
                        BLUE_BRIGHT,
                    );
                }
            }

            let fill_w = bar_w * cur_ratio;
            if fill_w > 0.5 {
                let fill_rect = egui::Rect::from_min_size(bg_line.min, Vec2::new(fill_w, track_h));
                ui.painter()
                    .rect_filled(fill_rect, Rounding::same(3.0_f32), BLUE_NEON);
            }
            if is_hov {
                ui.painter()
                    .circle_filled(egui::pos2(cur_x, bg_line.center().y), 5.5, BLUE_BRIGHT);
            }
            ui.add_space(20.0);

            let ctrl_total_w = 38.0 * 4.0 + 46.0 + 12.0 * 4.0;
            let (_, ctrl_rect) = ui.allocate_space(Vec2::new(ctrl_total_w, 48.0));
            let cy = ctrl_rect.center().y;
            let mut x = ctrl_rect.left();

            let like_r = egui::Rect::from_center_size(egui::pos2(x + 19.0, cy), Vec2::splat(38.0));
            if draw_media_icon_btn(ui, "b_like", like_r, MediaIcon::Heart(liked), false).clicked() {
                self.toggle_current_like();
            }
            x += 50.0;

            let prev_r = egui::Rect::from_center_size(egui::pos2(x + 19.0, cy), Vec2::splat(38.0));
            if draw_media_icon_btn(ui, "b_prev", prev_r, MediaIcon::Prev, false).clicked() {
                self.prev_or_restart(ctx);
            }
            x += 50.0;

            let play_r = egui::Rect::from_center_size(egui::pos2(x + 23.0, cy), Vec2::splat(46.0));
            let p_icon = if self.is_playing {
                MediaIcon::Pause
            } else {
                MediaIcon::Play
            };
            if draw_media_icon_btn(ui, "b_play", play_r, p_icon, true).clicked() {
                self.toggle_play_pause(ctx);
            }
            x += 58.0;

            let next_r = egui::Rect::from_center_size(egui::pos2(x + 19.0, cy), Vec2::splat(38.0));
            if draw_media_icon_btn(ui, "b_next", next_r, MediaIcon::Next, false).clicked() {
                self.step_queue(1, ctx);
            }
            x += 50.0;

            let vol_btn_r =
                egui::Rect::from_center_size(egui::pos2(x + 19.0, cy), Vec2::splat(38.0));
            let popup_r = egui::Rect::from_center_size(
                egui::pos2(vol_btn_r.center().x, vol_btn_r.top() - 62.0),
                Vec2::new(36.0, 108.0),
            );
            let hover_zone = vol_btn_r.union(popup_r).expand(6.0);
            let ptr_pos = ctx.input(|i| i.pointer.hover_pos());
            let dragging_v = ui.input(|i| i.pointer.primary_down())
                && ctx.data(|d| d.get_temp::<bool>(egui::Id::new("v_drag")).unwrap_or(false));

            if draw_media_icon_btn(
                ui,
                "b_vol",
                vol_btn_r,
                MediaIcon::Volume(self.config.volume),
                false,
            )
            .clicked()
            {
                self.config.volume = if self.config.volume > 0.01 { 0.0 } else { 0.7 };
                let _ = self.audio_tx.send(AudioCmd::SetVolume(self.config.volume));
                self.save_config();
            }

            if ptr_pos.map(|p| hover_zone.contains(p)).unwrap_or(false) || dragging_v {
                egui::Area::new(egui::Id::new("vol_hover_popup"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(popup_r.min)
                    .show(ctx, |ui| {
                        let (p_rect, p_resp) =
                            ui.allocate_exact_size(popup_r.size(), Sense::click_and_drag());
                        ui.painter().rect(
                            p_rect,
                            Rounding::same(18.0_f32),
                            BG_BAR,
                            Stroke::new(1.0_f32, BORDER_DARK_BLUE),
                        );

                        let top_y = p_rect.top() + 14.0;
                        let bot_y = p_rect.bottom() - 14.0;
                        let h = (bot_y - top_y).max(1.0);
                        let cx = p_rect.center().x;

                        if p_resp.drag_started() {
                            ctx.data_mut(|d| d.insert_temp(egui::Id::new("v_drag"), true));
                        }
                        if p_resp.clicked() || p_resp.dragged() {
                            if let Some(pos) = p_resp.interact_pointer_pos() {
                                self.config.volume = 1.0 - ((pos.y - top_y) / h).clamp(0.0, 1.0);
                                let _ = self.audio_tx.send(AudioCmd::SetVolume(self.config.volume));
                                self.save_config();
                            }
                        }
                        if !ui.input(|i| i.pointer.primary_down()) {
                            ctx.data_mut(|d| d.insert_temp(egui::Id::new("v_drag"), false));
                        }

                        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                        if scroll.abs() > 0.1 {
                            self.config.volume =
                                (self.config.volume + scroll * 0.002).clamp(0.0, 1.0);
                            let _ = self.audio_tx.send(AudioCmd::SetVolume(self.config.volume));
                            self.save_config();
                        }

                        let bg_b = egui::Rect::from_min_max(
                            egui::pos2(cx - 2.5, top_y),
                            egui::pos2(cx + 2.5, bot_y),
                        );
                        ui.painter()
                            .rect_filled(bg_b, Rounding::same(2.5_f32), BG_WIDGET);

                        let fill_h = h * self.config.volume.clamp(0.0, 1.0);
                        if fill_h > 0.5 {
                            let fb = egui::Rect::from_min_max(
                                egui::pos2(cx - 2.5, bot_y - fill_h),
                                egui::pos2(cx + 2.5, bot_y),
                            );
                            ui.painter()
                                .rect_filled(fb, Rounding::same(2.5_f32), BLUE_BRIGHT);
                        }
                    });
            }
        });
    }
}
enum MediaIcon {
    Heart(bool),
    Prev,
    Play,
    Pause,
    Next,
    Volume(f32),
}

fn draw_media_icon_btn(
    ui: &mut egui::Ui,
    id_str: &str,
    rect: egui::Rect,
    icon: MediaIcon,
    primary: bool,
) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(id_str), Sense::click());
    let hov = resp.hovered();
    let bg = if primary {
        if hov { BLUE_NEON } else { BLUE_DEEP }
    } else if hov {
        BG_HOVER
    } else {
        BG_WIDGET
    };
    let border = if primary || hov { BLUE_NEON } else { BORDER_DARK_BLUE };
    ui.painter().rect(
        rect,
        Rounding::same(rect.width() * 0.5),
        bg,
        Stroke::new(1.0_f32, border),
    );

    let fg = if primary || hov { BLUE_BRIGHT } else { TEXT_BLUE };
    let c = rect.center();
    let p = ui.painter();

    match icon {
        MediaIcon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - 3.5, c.y - 6.5),
                    egui::pos2(c.x - 3.5, c.y + 6.5),
                    egui::pos2(c.x + 7.0, c.y),
                ],
                fg,
                Stroke::NONE,
            ));
        }
        MediaIcon::Pause => {
            p.rect_filled(
                egui::Rect::from_center_size(egui::pos2(c.x - 4.0, c.y), Vec2::new(3.2, 13.0)),
                Rounding::same(1.0_f32),
                fg,
            );
            p.rect_filled(
                egui::Rect::from_center_size(egui::pos2(c.x + 4.0, c.y), Vec2::new(3.2, 13.0)),
                Rounding::same(1.0_f32),
                fg,
            );
        }
        MediaIcon::Prev => {
            p.rect_filled(
                egui::Rect::from_center_size(egui::pos2(c.x - 5.5, c.y), Vec2::new(2.2, 11.0)),
                Rounding::same(1.0_f32),
                fg,
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x + 5.5, c.y - 5.5),
                    egui::pos2(c.x + 5.5, c.y + 5.5),
                    egui::pos2(c.x - 3.0, c.y),
                ],
                fg,
                Stroke::NONE,
            ));
        }
        MediaIcon::Next => {
            p.rect_filled(
                egui::Rect::from_center_size(egui::pos2(c.x + 5.5, c.y), Vec2::new(2.2, 11.0)),
                Rounding::same(1.0_f32),
                fg,
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - 5.5, c.y - 5.5),
                    egui::pos2(c.x - 5.5, c.y + 5.5),
                    egui::pos2(c.x + 3.0, c.y),
                ],
                fg,
                Stroke::NONE,
            ));
        }
        MediaIcon::Heart(active) => {
            let h_col = if active {
                BLUE_NEON
            } else if hov {
                TEXT_BLUE
            } else {
                TEXT_GRAY
            };
            p.circle_filled(egui::pos2(c.x - 3.1, c.y - 2.0), 3.5, h_col);
            p.circle_filled(egui::pos2(c.x + 3.1, c.y - 2.0), 3.5, h_col);
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - 6.4, c.y - 0.5),
                    egui::pos2(c.x + 6.4, c.y - 0.5),
                    egui::pos2(c.x, c.y + 6.5),
                ],
                h_col,
                Stroke::NONE,
            ));
        }
        MediaIcon::Volume(v) => {
            p.rect_filled(
                egui::Rect::from_center_size(egui::pos2(c.x - 4.5, c.y), Vec2::new(3.2, 5.6)),
                Rounding::same(1.0_f32),
                fg,
            );
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - 3.5, c.y - 2.8),
                    egui::pos2(c.x + 1.5, c.y - 6.2),
                    egui::pos2(c.x + 1.5, c.y + 6.2),
                    egui::pos2(c.x - 3.5, c.y + 2.8),
                ],
                fg,
                Stroke::NONE,
            ));
            if v > 0.02 {
                p.line_segment(
                    [egui::pos2(c.x + 4.5, c.y - 3.5), egui::pos2(c.x + 4.5, c.y + 3.5)],
                    Stroke::new(1.6_f32, fg),
                );
            } else {
                p.line_segment(
                    [egui::pos2(c.x + 3.5, c.y - 2.5), egui::pos2(c.x + 7.0, c.y + 2.5)],
                    Stroke::new(1.5_f32, TEXT_GRAY),
                );
            }
        }
    }
    resp
}
fn draw_side_nav_btn(ui: &mut egui::Ui, _id: &str, kind: u8, active: bool, tip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
    let resp = resp.on_hover_text(tip);
    let bg = if active {
        BLUE_DEEP
    } else if resp.hovered() {
        BG_HOVER
    } else {
        BG_WIDGET
    };
    let border = if active || resp.hovered() { BLUE_NEON } else { BORDER_DARK_BLUE };
    ui.painter()
        .rect(rect, Rounding::same(8.0_f32), bg, Stroke::new(1.0_f32, border));

    let col = if active || resp.hovered() { BLUE_BRIGHT } else { TEXT_BLUE };
    let c = rect.center();
    let p = ui.painter();
    let st = Stroke::new(1.6_f32, col);

    match kind {
        0 => {
            for y in [-5.0_f32, 0.0, 5.0] {
                p.line_segment(
                    [egui::pos2(c.x - 6.5, c.y + y), egui::pos2(c.x + 6.5, c.y + y)],
                    st,
                );
            }
        }
        1 => {
            let r = egui::Rect::from_center_size(egui::pos2(c.x, c.y + 1.0), Vec2::new(14.0, 10.0));
            p.rect_stroke(r, Rounding::same(2.0_f32), st);
            p.line_segment(
                [egui::pos2(r.left() + 1.0, r.top() - 2.0), egui::pos2(r.left() + 6.0, r.top() - 2.0)],
                st,
            );
        }
        _ => {
            p.circle_stroke(c, 5.5, st);
            p.circle_filled(c, 2.0, col);
        }
    }
    resp.clicked()
}

fn eye_toggle_btn(ui: &mut egui::Ui, open: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(28.0, 26.0), Sense::click());
    let bg = if resp.hovered() { BG_HOVER } else { BG_WIDGET };
    ui.painter()
        .rect(rect, Rounding::same(5.0_f32), bg, Stroke::new(1.0_f32, BORDER_DARK_BLUE));
    let c = rect.center();
    let col = if open { BLUE_BRIGHT } else { TEXT_GRAY };
    ui.painter().circle_stroke(c, 4.5, Stroke::new(1.4_f32, col));
    if open {
        ui.painter().circle_filled(c, 1.8, col);
    } else {
        ui.painter().line_segment(
            [egui::pos2(c.x - 5.0, c.y + 5.0), egui::pos2(c.x + 5.0, c.y - 5.0)],
            Stroke::new(1.4_f32, col),
        );
    }
    resp.clicked()
}

fn win_bar_btn(ui: &mut egui::Ui, kind: u8) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(26.0, 22.0), Sense::click());
    let bg = if resp.hovered() { BG_HOVER } else { BG_WIDGET };
    ui.painter().rect_filled(rect, Rounding::same(4.0_f32), bg);
    let col = if resp.hovered() { BLUE_BRIGHT } else { TEXT_GRAY };
    let c = rect.center();
    let s = Stroke::new(1.4_f32, col);
    match kind {
        0 => {
            ui.painter()
                .line_segment([egui::pos2(c.x - 4.5, c.y + 2.0), egui::pos2(c.x + 4.5, c.y + 2.0)], s);
        }
        1 => {
            ui.painter()
                .rect_stroke(egui::Rect::from_center_size(c, Vec2::splat(8.5)), Rounding::same(1.0_f32), s);
        }
        _ => {
            ui.painter()
                .line_segment([egui::pos2(c.x - 4.0, c.y - 4.0), egui::pos2(c.x + 4.0, c.y + 4.0)], s);
            ui.painter()
                .line_segment([egui::pos2(c.x + 4.0, c.y - 4.0), egui::pos2(c.x - 4.0, c.y + 4.0)], s);
        }
    }
    resp
}
fn list_item_row(ui: &mut egui::Ui, text: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let bg = if selected {
        BG_HOVER
    } else if resp.hovered() {
        BG_WIDGET
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, Rounding::same(5.0_f32), bg);
    if selected {
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + 5.0),
            Vec2::new(3.0, rect.height() - 10.0),
        );
        ui.painter().rect_filled(bar, Rounding::same(1.5_f32), BLUE_NEON);
    }
    let col = if selected {
        BLUE_BRIGHT
    } else if resp.hovered() {
        TEXT_BLUE
    } else {
        TEXT_GRAY
    };
    let clipped: String = text.chars().take(33).collect();
    ui.painter().text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        clipped,
        FontId::proportional(12.2),
        col,
    );
    resp.clicked()
}

fn pill_action_btn(ui: &mut egui::Ui, label: &str, accent: bool) -> bool {
    let w = (label.chars().count() as f32 * 7.2 + 22.0).max(95.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 28.0), Sense::click());
    let bg = if accent {
        if resp.hovered() { BLUE_NEON } else { BLUE_DEEP }
    } else if resp.hovered() {
        BG_HOVER
    } else {
        BG_WIDGET
    };
    let border = if accent || resp.hovered() { BLUE_NEON } else { BORDER_DARK_BLUE };
    ui.painter()
        .rect(rect, Rounding::same(14.0_f32), bg, Stroke::new(1.0_f32, border));
    let col = if accent || resp.hovered() { BLUE_BRIGHT } else { TEXT_BLUE };
    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(11.5), col);
    resp.clicked()
}

fn full_width_btn(ui: &mut egui::Ui, label: &str, height: f32, accent: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let bg = if accent {
        if resp.hovered() { BLUE_NEON } else { BLUE_DEEP }
    } else if resp.hovered() {
        BG_HOVER
    } else {
        BG_WIDGET
    };
    let border = if accent || resp.hovered() { BLUE_NEON } else { BORDER_DARK_BLUE };
    ui.painter()
        .rect(rect, Rounding::same(6.0_f32), bg, Stroke::new(1.0_f32, border));
    let col = if accent || resp.hovered() { BLUE_BRIGHT } else { TEXT_BLUE };
    ui.painter()
        .text(rect.center(), egui::Align2::CENTER_CENTER, label, FontId::proportional(12.0), col);
    resp.clicked()
}

fn small_text_btn(ui: &mut egui::Ui, label: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(62.0, 18.0), Sense::click());
    let col = if resp.hovered() { BLUE_BRIGHT } else { TEXT_GRAY };
    ui.painter().text(
        rect.right_center(),
        egui::Align2::RIGHT_CENTER,
        label,
        FontId::proportional(10.5),
        col,
    );
    resp.clicked()
}

fn small_search_btn(ui: &mut egui::Ui, label: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::click());
    let bg = if resp.hovered() { BLUE_DEEP } else { BG_WIDGET };
    let border = if resp.hovered() { BLUE_NEON } else { BORDER_DARK_BLUE };
    ui.painter().rect(rect, Rounding::same(5.0_f32), bg, Stroke::new(1.0_f32, border));
    let col = if resp.hovered() { BLUE_BRIGHT } else { TEXT_BLUE };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        col,
    );
    resp.clicked()
}

fn dark_text_input(
    ui: &mut egui::Ui,
    width: f32,
    text: &mut String,
    hint: &str,
    password: bool,
) -> egui::Response {
    let mut resp_opt = None;
    egui::Frame::none()
        .fill(BG_VOID)
        .stroke(Stroke::new(1.0_f32, BORDER_DARK_BLUE))
        .rounding(Rounding::same(6.0_f32))
        .inner_margin(egui::Margin::symmetric(8.0, 4.0))
        .show(ui, |ui| {
            let inner_w = (width - 18.0).max(40.0);
            let r = ui.add_sized(
                [inner_w, 18.0],
                egui::TextEdit::singleline(text)
                    .frame(false)
                    .password(password)
                    .text_color(BLUE_BRIGHT)
                    .hint_text(RichText::new(hint).color(TEXT_GRAY)),
            );
            if r.has_focus() || r.hovered() {
                ui.painter().rect_stroke(
                    r.rect.expand2(Vec2::new(8.0, 4.0)),
                    Rounding::same(6.0_f32),
                    Stroke::new(1.0_f32, BLUE_NEON),
                );
            }
            resp_opt = Some(r);
        });
    resp_opt.unwrap()
}

fn artist_card_row(ui: &mut egui::Ui, name: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let hov = resp.hovered();
    if hov {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let bg = if hov { BLUE_DEEP } else { BG_WIDGET };
    let border = if hov { BLUE_NEON } else { BORDER_DARK_BLUE };
    ui.painter().rect(rect, Rounding::same(6.0_f32), bg, Stroke::new(1.0_f32, border));

    let badge_c = egui::pos2(rect.left() + 15.0, rect.center().y);
    ui.painter().circle_filled(badge_c, 8.0, if hov { BLUE_NEON } else { BG_VOID });
    ui.painter().circle_stroke(badge_c, 8.0, Stroke::new(1.0_f32, BLUE_NEON));
    ui.painter().circle_filled(badge_c, 2.5, BLUE_BRIGHT);

    let col = if hov { BLUE_BRIGHT } else { TEXT_BLUE };
    let clipped: String = name.chars().take(25).collect();
    ui.painter().text(
        egui::pos2(rect.left() + 30.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        clipped,
        FontId::proportional(12.5),
        col,
    );

    if hov {
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "->",
            FontId::proportional(12.0),
            BLUE_BRIGHT,
        );
    }
    resp.clicked()
}

fn format_time(sec: f32) -> String {
    let total = sec.max(0.0) as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn extract_token(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(pos) = trimmed.find("access_token=") {
        let after = &trimmed[pos + "access_token=".len()..];
        let end = after.find('&').unwrap_or(after.len());
        return after[..end].to_string();
    }
    trimmed.to_string()
}
enum KeyCapture {
    None,
    Cancelled,
    Combo(String),
}

fn capture_key_combo(ctx: &egui::Context) -> KeyCapture {
    ctx.input(|i| {
        for event in &i.events {
            if let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                if *key == egui::Key::Escape {
                    return KeyCapture::Cancelled;
                }

                let mut parts = Vec::new();
                if modifiers.ctrl {
                    parts.push("Ctrl".to_string());
                }
                if modifiers.alt {
                    parts.push("Alt".to_string());
                }
                if modifiers.shift {
                    parts.push("Shift".to_string());
                }

                let is_numpad = physical_key.is_none()
                    || matches!(
                        (*key, physical_key),
                        (egui::Key::Insert, Some(egui::Key::Num0))
                            | (egui::Key::End, Some(egui::Key::Num1))
                            | (egui::Key::ArrowDown, Some(egui::Key::Num2))
                            | (egui::Key::PageDown, Some(egui::Key::Num3))
                            | (egui::Key::ArrowLeft, Some(egui::Key::Num4))
                            | (egui::Key::ArrowRight, Some(egui::Key::Num6))
                            | (egui::Key::Home, Some(egui::Key::Num7))
                            | (egui::Key::ArrowUp, Some(egui::Key::Num8))
                            | (egui::Key::PageUp, Some(egui::Key::Num9))
                    );

                let chosen = physical_key.unwrap_or(*key);
                let name = match chosen {
                    egui::Key::Num0 => if is_numpad { "Numpad0" } else { "Digit0" }.to_string(),
                    egui::Key::Num1 => if is_numpad { "Numpad1" } else { "Digit1" }.to_string(),
                    egui::Key::Num2 => if is_numpad { "Numpad2" } else { "Digit2" }.to_string(),
                    egui::Key::Num3 => if is_numpad { "Numpad3" } else { "Digit3" }.to_string(),
                    egui::Key::Num4 => if is_numpad { "Numpad4" } else { "Digit4" }.to_string(),
                    egui::Key::Num5 => if is_numpad { "Numpad5" } else { "Digit5" }.to_string(),
                    egui::Key::Num6 => if is_numpad { "Numpad6" } else { "Digit6" }.to_string(),
                    egui::Key::Num7 => if is_numpad { "Numpad7" } else { "Digit7" }.to_string(),
                    egui::Key::Num8 => if is_numpad { "Numpad8" } else { "Digit8" }.to_string(),
                    egui::Key::Num9 => if is_numpad { "Numpad9" } else { "Digit9" }.to_string(),
                    other => format!("{:?}", other),
                };

                parts.push(name);
                return KeyCapture::Combo(parts.join("+"));
            }
        }
        KeyCapture::None
    })
}

fn build_window_icon() -> egui::IconData {
    let mut rgba = vec![0u8; 64 * 64 * 4];
    for y in 0..64 {
        for x in 0..64 {
            let idx = (y * 64 + x) * 4;
            let dx = x as f32 - 31.5;
            let dy = y as f32 - 31.5;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 30.0 {
                if (dist - 21.0).abs() <= 4.2 || dist <= 6.5 {
                    rgba[idx] = 0;
                    rgba[idx + 1] = 145;
                    rgba[idx + 2] = 255;
                    rgba[idx + 3] = 255;
                } else {
                    rgba[idx] = 5;
                    rgba[idx + 1] = 9;
                    rgba[idx + 2] = 16;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }
    egui::IconData {
        rgba,
        width: 64,
        height: 64,
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ostinato")
            .with_icon(Arc::new(build_window_icon()))
            .with_decorations(false)
            .with_resizable(true)
            .with_inner_size([960.0, 610.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Ostinato",
        options,
        Box::new(|cc| Ok(Box::new(OstinatoApp::new(cc)))),
    )
}

