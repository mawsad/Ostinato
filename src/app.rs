use crate::api::*;
use crate::audio::{read_cached_mp3, sanitize_filename, spawn_audio_engine, AudioCmd, SharedAudioState};
use crate::discord::{spawn_discord_rpc, DiscordCmd};
use crate::models::*;
use crate::providers::*;
use crate::theme::*;
use crate::ui_widgets::*;
use crate::win_sys::*;
use eframe::egui::{self, Color32, FontId, RichText, Rounding, Sense, Stroke, Vec2};
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use rand::seq::SliceRandom;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tray_icon::{menu::MenuEvent, MouseButton, MouseButtonState, TrayIcon, TrayIconEvent};

pub struct OstinatoApp {
    pub config: AppConfig,
    pub skin_assets: LoadedSkinAssets,
    pub available_skins: Vec<String>,
    pub new_skin_name: String,
    pub smooth_beat: f32,

    pub screen: AppScreen,
    pub active_tab: LeftTab,
    pub playback_ctx: PlaybackContext,

    pub auth_rx: Option<Receiver<AuthMsg>>,
    pub device_code_text: Option<String>,
    pub device_verify_url: Option<String>,
    pub auth_status: String,
    pub manual_mode: bool,
    pub copied_feedback_until: Option<Instant>,

    pub api_tx: Sender<ApiMsg>,
    pub api_rx: Receiver<ApiMsg>,
    pub ym_uid: Option<u64>,
    pub liked_ids: Vec<String>,
    pub sync_status: String,

    pub audio_tx: Sender<AudioCmd>,
    pub audio_state: Arc<SharedAudioState>,
    pub play_seq: Arc<AtomicU64>,
    pub preloaded_audio: Option<(String, Arc<Vec<u8>>)>,
    pub preloading_track_id: Option<String>,
    pub crossfade_triggered_for: Option<String>,

    pub hotkey_mgr: Option<GlobalHotKeyManager>,
    pub registered_hotkeys: Vec<HotKey>,
    pub hotkey_map: HashMap<u32, BindAction>,
    pub bg_hotkey_map: Arc<Mutex<HashMap<u32, BindAction>>>,
    pub hk_rx: Receiver<u32>,
    pub _tray_icon: Option<TrayIcon>,
    pub tray_wake_rx: Receiver<()>,

    pub token_input: String,
    pub show_token: bool,
    pub rebinding_action: Option<String>,

    pub is_playing: bool,
    pub is_sleep_mode: bool,
    pub current_pos: f32,
    pub current_track: Option<TrackItem>,
    pub track_audio_loaded: bool,
    pub track_transition_start: Instant,

    pub show_lyrics: bool,
    pub lyrics_track_id: Option<String>,
    pub lyrics_loading: bool,
    pub lyrics_synced: bool,
    pub lyrics_lines: Vec<LyricLine>,

    pub wave_buffer: VecDeque<TrackItem>,
    pub wave_loading: bool,
    pub wave_batch_id: Option<String>,
    pub wave_seen_ids: VecDeque<String>,
    pub ym_cloud_playlists: Vec<PlaylistEntry>,
    pub playlists: Vec<PlaylistEntry>,
    pub custom_playlists: Vec<CustomMixPlaylist>,
    pub new_mix_name: String,
    pub pending_mix_track: Option<TrackItem>,
    pub selected_playlist: usize,
    pub playlist_tracks: Vec<TrackItem>,
    pub active_pool: Vec<TrackItem>,
    pub current_pool_idx: Option<usize>,
    pub shuffle_order: Vec<usize>,
    pub shuffle_ptr: usize,

    pub user_queue: VecDeque<TrackItem>,
    pub history: VecDeque<TrackItem>,
    pub queue_subtab_history: bool,

    pub search_provider: TrackProvider,
    pub search_query: String,
    pub search_artists: Vec<ArtistItem>,
    pub search_albums: Vec<AlbumItem>,
    pub search_tracks: Vec<TrackItem>,
    pub search_header: String,
    pub viewing_sublist: bool,
    pub last_search_tracks: Vec<TrackItem>,
    pub last_search_header: String,
    pub local_tracks: Vec<TrackItem>,
    pub discord_tx: Sender<DiscordCmd>,
}


impl OstinatoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let config = AppConfig::load();
        let (pal, skin_assets) = load_theme_and_assets(&cc.egui_ctx, &config.active_theme, Some(config.custom_palette));
        set_active_palette(pal);
        setup_ostinato_theme(&cc.egui_ctx);
        apply_skin_font(&cc.egui_ctx, skin_assets.font_path.as_deref());


        let has_any = !config.token.trim().is_empty();
        let screen = if has_any { AppScreen::MainPlayer } else { AppScreen::Onboarding };

        let token_input = config.token.clone();
        let (api_tx, api_rx) = mpsc::channel();
        let (audio_tx, audio_state) = spawn_audio_engine();
        let discord_tx = spawn_discord_rpc();
        let discord_tx = spawn_discord_rpc();
        let hotkey_mgr = GlobalHotKeyManager::new().ok();

        let (hk_tx, hk_rx) = mpsc::channel::<u32>();
        let ctx_wakeup = cc.egui_ctx.clone();
        let bg_hotkey_map = Arc::new(Mutex::new(HashMap::<u32, BindAction>::new()));
        let bg_hk_ref = Arc::clone(&bg_hotkey_map);

        thread::spawn(move || {
            let receiver = GlobalHotKeyEvent::receiver();
            while let Ok(ev) = receiver.recv() {
                if ev.state == HotKeyState::Pressed {
                    let act = bg_hk_ref.lock().ok().and_then(|m| m.get(&ev.id).copied());
                    if act == Some(BindAction::Sleep) { force_wake_window_win32(); }
                    let _ = hk_tx.send(ev.id);
                    ctx_wakeup.request_repaint();
                }
            }
        });

        let (tray_tx, tray_wake_rx) = mpsc::channel::<()>();
        let (_tray_icon, show_id, quit_id) = build_ostinato_tray();
        let tray_tx_m = tray_tx.clone();
        let ctx_m = cc.egui_ctx.clone();
        thread::spawn(move || {
            let rx = MenuEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if ev.id == quit_id {
                    ctx_m.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                else if ev.id == show_id {
                    force_wake_window_win32();
                    let _ = tray_tx_m.send(());
                    ctx_m.request_repaint();
                }
            }
        });
        let ctx_tr = cc.egui_ctx.clone();
        thread::spawn(move || {
            let rx = TrayIconEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                    force_wake_window_win32();
                    let _ = tray_tx.send(());
                    ctx_tr.request_repaint();
                }
            }
        });

        let st_watch = Arc::clone(&audio_state);
        let ctx_aud = cc.egui_ctx.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(320));
            if st_watch.finished_seq.load(Ordering::Relaxed) != 0 { ctx_aud.request_repaint(); }
        });

        let custom_playlists = load_custom_playlists();
        let available_skins = list_available_skins();

        let mut app = Self {
            config, skin_assets, available_skins, new_skin_name: "MyAnimeSkin".into(), smooth_beat: 0.0,
            screen, active_tab: LeftTab::Playlists, playback_ctx: PlaybackContext::Playlist,
            auth_rx: None, device_code_text: None, device_verify_url: None, auth_status: String::new(),
            manual_mode: false, copied_feedback_until: None,
            api_tx, api_rx, ym_uid: None, liked_ids: Vec::new(), sync_status: String::new(),
            audio_tx, audio_state, play_seq: Arc::new(AtomicU64::new(0)),
            preloaded_audio: None, preloading_track_id: None, crossfade_triggered_for: None,
            hotkey_mgr, registered_hotkeys: Vec::new(), hotkey_map: HashMap::new(), bg_hotkey_map, hk_rx, _tray_icon, tray_wake_rx,
            token_input, show_token: false, rebinding_action: None,
            is_playing: false, is_sleep_mode: false, current_pos: 0.0, current_track: None, track_audio_loaded: false,
            track_transition_start: Instant::now(),
            show_lyrics: false, lyrics_track_id: None, lyrics_loading: false, lyrics_synced: false, lyrics_lines: Vec::new(),
            wave_buffer: VecDeque::new(), wave_loading: false, wave_batch_id: None, wave_seen_ids: VecDeque::new(),
            ym_cloud_playlists: vec![PlaylistEntry { title: "Мне нравится [YM]".into(), source: PlaylistSource::Likes }],
            playlists: Vec::new(), custom_playlists, new_mix_name: String::new(), pending_mix_track: None,
            selected_playlist: 0, playlist_tracks: Vec::new(), active_pool: Vec::new(), current_pool_idx: None,
            shuffle_order: Vec::new(), shuffle_ptr: 0,
            user_queue: VecDeque::new(), history: VecDeque::new(), queue_subtab_history: false,
            search_provider: TrackProvider::Yandex, search_query: String::new(), search_artists: Vec::new(),
            search_albums: Vec::new(), search_tracks: Vec::new(), search_header: String::new(),
            viewing_sublist: false, last_search_tracks: Vec::new(), last_search_header: String::new(),
            local_tracks: Vec::new(),
            discord_tx,

        };

        app.rebuild_combined_playlists();
        app.refresh_local_folder();
        app.reload_global_hotkeys();
      if !app.config.token.trim().is_empty() {
          app.trigger_ym_init(cc.egui_ctx.clone());
      } else {
          app.search_provider = TrackProvider::SoundCloud;
          if !app.local_tracks.is_empty() {
              app.active_tab = LeftTab::LocalFiles;
          }
      }

        app
    }

    pub fn apply_selected_theme(&mut self, name: &str, ctx: &egui::Context) {
        self.config.active_theme = name.to_string();
        let (pal, assets) = load_theme_and_assets(ctx, name, Some(self.config.custom_palette));
        if name != "Custom" { self.config.custom_palette = pal; }
        apply_skin_font(ctx, assets.font_path.as_deref());
        self.skin_assets = assets;
        set_active_palette(pal);
        setup_ostinato_theme(ctx);
        self.config.save();
    }


    pub fn ensure_editable_skin_dir(&mut self) -> PathBuf {
        if !self.config.active_theme.starts_with("Skin: ") {
            let clean = if self.new_skin_name.trim().is_empty() { "StudioSkin" } else { self.new_skin_name.trim() };
            let pal = active_palette();
            if let Ok(dir) = save_skin_folder(clean, &pal) {
                if let Some(n) = dir.file_name().and_then(|s| s.to_str()) {
                    self.config.active_theme = format!("Skin: {}", n);
                    self.available_skins = list_available_skins();
                    self.config.save();
                    return dir;
                }
            }
        }
        active_skin_folder(&self.config.active_theme)
    }

    pub fn reload_current_skin_assets(&mut self, ctx: &egui::Context) {
        let dir = active_skin_folder(&self.config.active_theme);
        if self.config.active_theme.starts_with("Skin: ") {
            if let Ok(json) = serde_json::to_string_pretty(&active_palette()) {
                let _ = std::fs::write(dir.join("skin.json"), json);
            }
        }
        let assets = load_skin_assets_from_dir(ctx, &dir);
        apply_skin_font(ctx, assets.font_path.as_deref());
        self.skin_assets = assets;
        ctx.request_repaint();
    }



    pub fn rebuild_combined_playlists(&mut self) {
        let mut list = Vec::new();
        for mx in &self.custom_playlists {
            list.push(PlaylistEntry {
                title: format!("★ {} [MIX]", mx.title),
                source: PlaylistSource::CustomMix(mx.id.clone()),
            });
        }
        for pl in &self.ym_cloud_playlists {
            list.push(pl.clone());
        }
        self.playlists = list;
        if self.selected_playlist >= self.playlists.len() {
            self.selected_playlist = 0;
        }
    }
    pub fn reload_global_hotkeys(&mut self) {
        let Some(mgr) = &self.hotkey_mgr else { return };
        for hk in self.registered_hotkeys.drain(..) { let _ = mgr.unregister(hk); }
        self.hotkey_map.clear();
        for (combo, act) in [
            (&self.config.keybinds.play_pause, BindAction::PlayPause),
            (&self.config.keybinds.next_track, BindAction::Next),
            (&self.config.keybinds.prev_track, BindAction::Prev),
            (&self.config.keybinds.like_track, BindAction::Like),
            (&self.config.keybinds.shake_wave, BindAction::ShakeWave),
            (&self.config.keybinds.sleep_mode, BindAction::Sleep),
            (&self.config.keybinds.toggle_shuffle, BindAction::ToggleShuffle),
            (&self.config.keybinds.cycle_repeat, BindAction::CycleRepeat),
            (&self.config.keybinds.vol_up, BindAction::VolUp),
            (&self.config.keybinds.vol_down, BindAction::VolDown),
        ] {
            if let Some(hk) = parse_hotkey_str(combo) {
                if mgr.register(hk).is_ok() {
                    self.hotkey_map.insert(hk.id(), act);
                    self.registered_hotkeys.push(hk);
                }
            }
        }
        if let Ok(mut bg) = self.bg_hotkey_map.lock() { *bg = self.hotkey_map.clone(); }
    }

    pub fn adjust_volume_step(&mut self, delta: f32) {
        self.config.volume = (self.config.volume + delta).clamp(0.0, 1.0);
        let _ = self.audio_tx.send(AudioCmd::SetVolume(self.config.volume));
        self.config.save();
    }
    pub fn sync_discord_presence(&self) {
        if let Some(tr) = &self.current_track {
            if self.is_playing {
                let _ = self.discord_tx.send(DiscordCmd::Playing {
                    title: tr.title.clone(),
                    artist: tr.artist.clone(),
                    provider: tr.provider.short_tag(),
                    duration_sec: tr.duration_sec,
                    pos_sec: self.current_pos,
                    cover_url: tr.cover_url.clone(),
                });
            } else {
                let _ = self.discord_tx.send(DiscordCmd::Paused {
                    title: tr.title.clone(),
                    artist: tr.artist.clone(),
                });
            }
        } else {
            let _ = self.discord_tx.send(DiscordCmd::Clear);
        }
    }


    pub fn refresh_local_folder(&mut self) {
        if let Some(f) = &self.config.local_folder {
            self.local_tracks = crate::audio::scan_folder_tracks(f);
        } else {
            self.local_tracks.clear();
        }
    }

    pub fn create_mix_playlist(&mut self, title: &str) {
        let t = title.trim();
        if t.is_empty() { return; }
        let id = format!("mix_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(1));
        self.custom_playlists.push(CustomMixPlaylist { id, title: t.to_string(), tracks: Vec::new() });
        save_custom_playlists(&self.custom_playlists);
        self.rebuild_combined_playlists();
    }

    pub fn add_track_to_mix(&mut self, mix_id: &str, track: TrackItem) {
        if let Some(mx) = self.custom_playlists.iter_mut().find(|m| m.id == mix_id) {
            if !mx.tracks.iter().any(|x| x.id == track.id) {
                mx.tracks.push(track);
                let mx_title = mx.title.clone();
                let updated = mx.tracks.clone();
                save_custom_playlists(&self.custom_playlists);
                self.sync_status = format!("Добавлено в «{}»", mx_title);
                if let Some(sel) = self.playlists.get(self.selected_playlist) {
                    if sel.source == PlaylistSource::CustomMix(mix_id.to_string()) {
                        self.playlist_tracks = updated;
                    }
                }
            }
        }
    }


    pub fn trigger_ym_init(&mut self, ctx: egui::Context) {
        let tok = self.config.token.trim().to_string();
        if tok.is_empty() { return; }
        self.sync_status = "Синхронизация Яндекса...".into();
        let tx = self.api_tx.clone();
        thread::spawn(move || {
            match fetch_ym_init(&tok) {
                Ok((uid, playlists, liked_ids, initial_tracks)) => {
                    let total = liked_ids.len();
                    let liked_c = liked_ids.clone();
                    let _ = tx.send(ApiMsg::InitDone { uid, playlists, liked_ids, initial_tracks });
                    ctx.request_repaint();
                    if total > 150 {
                        let cl = shared_ym_client();
                        let base = ym_api_base();
                        let auth = format!("OAuth {}", tok);
                        for ch in liked_c[150..].chunks(150) {
                            if let Ok(more) = fetch_tracks_chunk(cl, &base, &auth, ch, &liked_c) {
                                if !more.is_empty() {
                                    let _ = tx.send(ApiMsg::LikesChunkAppended { tracks: more });
                                    ctx.request_repaint();
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(ApiMsg::Error(e));
                    ctx.request_repaint();
                }
            }
        });
    }
    pub fn select_playlist(&mut self, idx: usize, ctx: &egui::Context) {
        if idx >= self.playlists.len() { return; }
        self.selected_playlist = idx;
        let src = self.playlists[idx].source.clone();

        match src {
            PlaylistSource::CustomMix(mix_id) => {
                if let Some(mx) = self.custom_playlists.iter().find(|m| m.id == mix_id) {
                    self.playlist_tracks = mx.tracks.clone();
                }
                return;
            }
            PlaylistSource::VkMyAudio => {
                return;
            }
            _ => {}
        }


        let Some(uid) = self.ym_uid else { return };
        let tok = self.config.token.clone();
        let liked = self.liked_ids.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        self.sync_status = "Загрузка плейлиста...".into();

        thread::spawn(move || {
            match fetch_playlist_tracks(&tok, uid, &src, &liked) {
                Ok(tracks) => { let _ = tx.send(ApiMsg::PlaylistLoaded { tracks }); }
                Err(e) => { let _ = tx.send(ApiMsg::Error(e)); }
            }
            ctx_c.request_repaint();
        });
    }

    pub fn trigger_search(&mut self, ctx: &egui::Context) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() { return; }
        let prov = self.search_provider;
        self.active_tab = LeftTab::Search;
        self.viewing_sublist = false;
        self.search_header = format!("Поиск [{}]: «{}»", prov.short_tag(), q);
        self.sync_status = format!("Поиск в {}...", prov.short_tag());

        let ym_tok = self.config.token.clone();
        let liked = self.liked_ids.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();

        thread::spawn(move || {
            let res = match prov {
                TrackProvider::Yandex => {
                    if ym_tok.trim().is_empty() {
                        Err("Подключите Яндекс во вкладке Аккаунты".into())
                    } else {
                        search_ym_all(&ym_tok, &q, &liked)
                    }
                }
                TrackProvider::SoundCloud => {
                    search_soundcloud(&q).map(|tr| (Vec::new(), Vec::new(), tr))
                }
                TrackProvider::Vk | TrackProvider::Local => {
                    Err("Не поддерживается".into())
                }
            };
            match res {
                Ok((artists, albums, tracks)) => {
                    let _ = tx.send(ApiMsg::SearchDone { artists, albums, tracks });
                }
                Err(e) => { let _ = tx.send(ApiMsg::Error(e)); }
            }
            ctx_c.request_repaint();
        });
    }

    pub fn open_artist(&mut self, artist_id: String, artist_name: String, ctx: &egui::Context) {
        let tok = self.config.token.clone();
        if tok.is_empty() { return; }
        self.active_tab = LeftTab::Search;
        self.viewing_sublist = true;
        self.search_header = format!("Популярные треки: {}", artist_name);
        self.sync_status = format!("Загрузка {}...", artist_name);
        let liked = self.liked_ids.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        thread::spawn(move || {
            match fetch_artist_tracks(&tok, &artist_id, &liked) {
                Ok(tracks) => { let _ = tx.send(ApiMsg::ArtistLoaded { artist_name, tracks }); }
                Err(e) => { let _ = tx.send(ApiMsg::Error(e)); }
            }
            ctx_c.request_repaint();
        });
    }

    pub fn open_album(&mut self, album_id: String, album_title: String, ctx: &egui::Context) {
        let tok = self.config.token.clone();
        if tok.is_empty() { return; }
        self.active_tab = LeftTab::Search;
        self.viewing_sublist = true;
        self.search_header = format!("Альбом: {}", album_title);
        self.sync_status = format!("Загрузка альбома {}...", album_title);
        let liked = self.liked_ids.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        thread::spawn(move || {
            match fetch_album_tracks(&tok, &album_id, &liked) {
                Ok(tracks) => { let _ = tx.send(ApiMsg::AlbumLoaded { album_title, tracks }); }
                Err(e) => { let _ = tx.send(ApiMsg::Error(e)); }
            }
            ctx_c.request_repaint();
        });
    }

    pub fn request_lyrics_for_current(&mut self, ctx: &egui::Context) {
        let Some(cur) = &self.current_track else { return };
        if self.lyrics_track_id.as_deref() == Some(cur.id.as_str()) { return; }
        let tid = cur.id.clone();
        let title = cur.title.clone();
        let artist = cur.artist.clone();
        let dur = cur.duration_sec;

        self.lyrics_track_id = Some(tid.clone());
        self.lyrics_lines.clear();
        self.lyrics_synced = false;

        let tok = self.config.token.clone();
        self.lyrics_loading = true;
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        thread::spawn(move || {
            match fetch_track_lyrics(&tok, &tid, &title, &artist, dur) {
                Ok((synced, lines)) => { let _ = tx.send(ApiMsg::LyricsReady { track_id: tid, synced, lines }); }
                Err(_) => { let _ = tx.send(ApiMsg::LyricsReady { track_id: tid, synced: false, lines: Vec::new() }); }
            }
            ctx_c.request_repaint();
        });
    }


    pub fn export_current_mp3(&mut self, ctx: &egui::Context) {
        let Some(tr) = self.current_track.clone() else { return };
        if tr.local_path.is_some() {
            self.sync_status = "Трек уже сохранён в локальной папке".into();
            return;
        }
        let target_dir: PathBuf = if let Some(dir) = &self.config.local_folder {
            dir.clone()
        } else if let Some(picked) = rfd::FileDialog::new().pick_folder() {
            self.config.local_folder = Some(picked.clone());
            self.config.save();
            picked
        } else {
            return;
        };

        let file_name = format!("{}.mp3", sanitize_filename(&format!("{} - {}", tr.artist, tr.title)));
        let full_path = target_dir.join(&file_name);
        let ym_tok = self.config.token.clone();
        let vk_tok = self.config.vk_token.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        self.sync_status = "Сохранение аудио...".into();

        thread::spawn(move || {
            match download_any_track_bytes(&tr, &ym_tok, &vk_tok) {
                Ok(bytes) => {
                    if std::fs::write(&full_path, &bytes).is_ok() {
                        let _ = tx.send(ApiMsg::ExportDone(format!("Сохранено: {}", file_name)));
                    } else {
                        let _ = tx.send(ApiMsg::Error("Ошибка записи файла".into()));
                    }
                }
                Err(e) => { let _ = tx.send(ApiMsg::Error(e)); }
            }
            ctx_c.request_repaint();
        });
    }
    pub fn push_to_history(&mut self, track: TrackItem) {
        if self.history.front().map(|t| t.id == track.id).unwrap_or(false) { return; }
        self.history.push_front(track);
        while self.history.len() > 80 { self.history.pop_back(); }
    }

    pub fn enqueue_next(&mut self, track: TrackItem) {
        self.user_queue.push_front(track);
        self.preloaded_audio = None;
        self.preloading_track_id = None;
    }

    pub fn enqueue_last(&mut self, track: TrackItem) {
        self.user_queue.push_back(track);
    }

    pub fn rebuild_shuffle_order(&mut self) {
        let len = self.active_pool.len();
        self.shuffle_order = (0..len).collect();
        if self.config.shuffle && len > 1 {
            let mut rng = rand::thread_rng();
            self.shuffle_order.shuffle(&mut rng);
            if let Some(cur) = self.current_pool_idx {
                if let Some(pos) = self.shuffle_order.iter().position(|&x| x == cur) {
                    self.shuffle_order.swap(0, pos);
                }
            }
        }
        self.shuffle_ptr = 0;
    }

    pub fn toggle_shuffle(&mut self) {
        if self.playback_ctx == PlaybackContext::Wave { return; }
        self.config.shuffle = !self.config.shuffle;
        self.config.save();
        self.rebuild_shuffle_order();
        self.preloaded_audio = None;
        self.preloading_track_id = None;
    }

    pub fn cycle_repeat(&mut self) {
        if self.playback_ctx == PlaybackContext::Wave { return; }
        self.config.repeat = match self.config.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        self.preloaded_audio = None;
        self.preloading_track_id = None;
        self.config.save();
    }

    pub fn start_track_from_pool(&mut self, pool: Vec<TrackItem>, idx: usize, ctx_kind: PlaybackContext, ctx: &egui::Context) {
        if idx >= pool.len() { return; }
        let tr = pool[idx].clone();
        self.playback_ctx = ctx_kind;
        self.active_pool = pool;
        self.current_pool_idx = Some(idx);
        self.rebuild_shuffle_order();
        if !self.config.shuffle { self.shuffle_ptr = idx; }
        self.play_track_direct(tr, true, false, ctx);
    }

    pub fn play_track_direct(&mut self, track: TrackItem, record_prev: bool, use_crossfade: bool, ctx: &egui::Context) {
        if record_prev {
            if let Some(prev) = self.current_track.take() { self.push_to_history(prev); }
        }
        let seq = self.play_seq.fetch_add(1, Ordering::SeqCst) + 1;
        self.current_pos = 0.0;
        self.crossfade_triggered_for = None;
        self.track_transition_start = Instant::now();
        self.current_track = Some(track.clone());
        self.track_audio_loaded = true;
        self.preloading_track_id = None;
        if self.show_lyrics { self.request_lyrics_for_current(ctx); }

        let cf = use_crossfade && self.config.crossfade;
        self.audio_state.finished_seq.store(0, Ordering::SeqCst);

        if self.playback_ctx == PlaybackContext::Wave && track.provider == TrackProvider::Yandex {
            if !self.wave_seen_ids.contains(&track.id) {
                self.wave_seen_ids.push_back(track.id.clone());
                while self.wave_seen_ids.len() > 250 { self.wave_seen_ids.pop_front(); }
            }
            let tok_fb = self.config.token.clone();
            let tid_fb = track.id.clone();
            let bid_fb = self.wave_batch_id.clone();
            thread::spawn(move || { send_wave_track_started(&tok_fb, &tid_fb, bid_fb.as_deref()); });
        }

        if let Some(path) = track.local_path {
            self.sync_status.clear();
            self.is_playing = true;
            self.sync_discord_presence();
            let _ = self.audio_tx.send(AudioCmd::LoadFile { seq, path, duration: track.duration_sec, vol: self.config.volume, crossfade: cf });
            return;
        }
        if let Some((cid, bytes)) = self.preloaded_audio.take() {
            if cid == track.id {
                self.sync_status.clear();
                self.is_playing = true;
                self.sync_discord_presence();
                let _ = self.audio_tx.send(AudioCmd::LoadBytes { seq, bytes, duration: track.duration_sec, vol: self.config.volume, crossfade: cf });
                return;
            }
        }
        if let Some(disk_b) = read_cached_mp3(&track.id) {
            self.sync_status.clear();
            self.is_playing = true;
            self.sync_discord_presence();
            let _ = self.audio_tx.send(AudioCmd::LoadBytes { seq, bytes: Arc::new(disk_b), duration: track.duration_sec, vol: self.config.volume, crossfade: cf });
            return;
        }

        self.sync_status = format!("Буферизация [{}]...", track.provider.short_tag());
        let ym_tok = self.config.token.clone();
        let vk_tok = self.config.vk_token.clone();
        let tx = self.api_tx.clone();
        let seq_ref = Arc::clone(&self.play_seq);
        let ctx_c = ctx.clone();

        thread::spawn(move || {
            match download_any_track_bytes(&track, &ym_tok, &vk_tok) {
                Ok(bytes) => {
                    if seq_ref.load(Ordering::SeqCst) == seq {
                        let _ = tx.send(ApiMsg::AudioReady { seq, track, bytes: Arc::new(bytes), crossfade: cf });
                    }
                }
                Err(e) => {
                    if seq_ref.load(Ordering::SeqCst) == seq { let _ = tx.send(ApiMsg::Error(e)); }
                }
            }
            ctx_c.request_repaint();
        });
    }
    pub fn peek_upcoming_track(&self) -> Option<TrackItem> {
        if self.playback_ctx != PlaybackContext::Wave && self.config.repeat == RepeatMode::One {
            return self.current_track.clone();
        }
        if let Some(qf) = self.user_queue.front() {
            return Some(qf.clone());
        }
        if self.playback_ctx == PlaybackContext::Wave {
            return self.wave_buffer.front().cloned();
        }
        let len = self.active_pool.len();
        if len == 0 {
            return None;
        }
        if self.config.shuffle && self.shuffle_order.len() == len {
            let np = self.shuffle_ptr + 1;
            if np < len {
                return self.shuffle_order.get(np).and_then(|&i| self.active_pool.get(i).cloned());
            }
            if self.config.repeat == RepeatMode::All {
                return self.shuffle_order.first().and_then(|&i| self.active_pool.get(i).cloned());
            }
            return None;
        }
        let cur = self.current_pool_idx.unwrap_or(0);
        if cur + 1 < len {
            self.active_pool.get(cur + 1).cloned()
        } else if self.config.repeat == RepeatMode::All {
            self.active_pool.first().cloned()
        } else {
            None
        }
    }


    pub fn maybe_preload_next(&mut self, ctx: &egui::Context) {
        let Some(cur) = &self.current_track else { return };
        let rem = cur.duration_sec - self.current_pos;
        if rem > 13.0 || rem < 0.4 { return; }
        let Some(next_tr) = self.peek_upcoming_track() else { return };
        if next_tr.local_path.is_some() { return; }
        if self.preloaded_audio.as_ref().map(|(id, _)| id == &next_tr.id).unwrap_or(false) { return; }
        if self.preloading_track_id.as_deref() == Some(next_tr.id.as_str()) { return; }

        let tid = next_tr.id.clone();
        self.preloading_track_id = Some(tid.clone());
        let ym_tok = self.config.token.clone();
        let vk_tok = self.config.vk_token.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();

        thread::spawn(move || {
            if let Ok(bytes) = download_any_track_bytes(&next_tr, &ym_tok, &vk_tok) {
                let _ = tx.send(ApiMsg::PreloadReady { track_id: tid, bytes: Arc::new(bytes) });
                ctx_c.request_repaint();
            }
        });
    }

    pub fn record_wave_track_id(&mut self, tid: &str) {
        if tid.is_empty() || tid.starts_with("local::") { return; }
        self.config.wave_recent_ids.retain(|x| x != tid);
        self.config.wave_recent_ids.insert(0, tid.to_string());
        if self.config.wave_recent_ids.len() > 40 { self.config.wave_recent_ids.truncate(40); }
        self.config.save();
    }

    pub fn launch_or_shake_wave(&mut self, ctx: &egui::Context) {
        let tok = self.config.token.clone();
        if tok.is_empty() {
            self.sync_status = "Для Моей волны подключите Яндекс во вкладке Аккаунты".into();
            return;
        }
        self.playback_ctx = PlaybackContext::Wave;
        self.wave_loading = true;
        self.sync_status = "Настройка потока Моей волны...".into();
        let prev_id = self.current_track.as_ref().map(|t| t.id.clone());
        let played = self.current_pos;
        let liked = self.liked_ids.clone();
        let bid = self.wave_batch_id.clone();
        let ws = self.config.wave_settings.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        thread::spawn(move || {
            match fetch_wave_batch(&tok, &liked, prev_id.as_deref(), played, &[], bid.as_deref(), Some(&ws)) {
                Ok((tracks, batch_id)) => {
                    let _ = tx.send(ApiMsg::WaveBatchReady { tracks, batch_id, replace_and_play: true, step_next_immediately: false });
                }
                Err(e) => { let _ = tx.send(ApiMsg::Error(e)); }
            }
            ctx_c.request_repaint();
        });
    }

    pub fn refill_wave_if_low(&mut self, step_immediately: bool, ctx: &egui::Context) {
        if self.wave_loading { return; }
        let tok = self.config.token.clone();
        if tok.is_empty() { return; }
        self.wave_loading = true;
        if step_immediately { self.sync_status = "Подгрузка Моей волны...".into(); }
        let prev_id = self.current_track.as_ref().map(|t| t.id.clone());
        let played = self.current_pos;
        let mut tail_ids = Vec::new();
        if let Some(cur) = &self.current_track { tail_ids.push(cur.id.clone()); }
        for item in &self.wave_buffer { tail_ids.push(item.id.clone()); }
        let liked = self.liked_ids.clone();
        let bid = self.wave_batch_id.clone();
        let tx = self.api_tx.clone();
        let ctx_c = ctx.clone();
        thread::spawn(move || {
            match fetch_wave_batch(&tok, &liked, prev_id.as_deref(), played, &tail_ids, bid.as_deref(), None) {
                Ok((tracks, batch_id)) => {
                    let _ = tx.send(ApiMsg::WaveBatchReady { tracks, batch_id, replace_and_play: false, step_next_immediately: step_immediately });
                }
                Err(e) => { let _ = tx.send(ApiMsg::Error(e)); }
            }
            ctx_c.request_repaint();
        });
    }
    pub fn toggle_play_pause(&mut self, ctx: &egui::Context) {
        if let Some(cur) = self.current_track.clone() {
            if !self.track_audio_loaded {
                self.play_track_direct(cur, false, false, ctx);
                return;
            }
        } else {
            if self.active_tab == LeftTab::Wave {
                self.launch_or_shake_wave(ctx);
            } else if self.active_tab == LeftTab::LocalFiles && !self.local_tracks.is_empty() {
                let pool = self.local_tracks.clone();
                self.start_track_from_pool(pool, 0, PlaybackContext::Local, ctx);
            } else if !self.playlist_tracks.is_empty() {
                let pool = self.playlist_tracks.clone();
                self.start_track_from_pool(pool, 0, PlaybackContext::Playlist, ctx);
            }
            return;
        }
        self.is_playing = !self.is_playing;
        let cmd = if self.is_playing { AudioCmd::Play } else { AudioCmd::Pause };
        let _ = self.audio_tx.send(cmd);
        self.sync_discord_presence();
    }

    pub fn step_next(&mut self, auto_finished: bool, use_crossfade: bool, ctx: &egui::Context) {
        if auto_finished && self.playback_ctx != PlaybackContext::Wave && self.config.repeat == RepeatMode::One {
            if let Some(cur) = self.current_track.clone() {
                self.play_track_direct(cur, false, use_crossfade, ctx);
                return;
            }
        }
        if let Some(queued) = self.user_queue.pop_front() {
            self.play_track_direct(queued, true, use_crossfade, ctx);
            return;
        }
        if self.playback_ctx == PlaybackContext::Wave {
            if let Some(next_wave) = self.wave_buffer.pop_front() {
                self.record_wave_track_id(&next_wave.id);
                self.play_track_direct(next_wave, true, use_crossfade, ctx);
                if self.wave_buffer.len() <= 2 { self.refill_wave_if_low(false, ctx); }
            } else {
                self.refill_wave_if_low(true, ctx);
            }
            return;
        }

        let len = self.active_pool.len();
        if len == 0 { return; }

        if self.config.shuffle {
            if self.shuffle_order.len() != len { self.rebuild_shuffle_order(); }
            if self.shuffle_ptr + 1 < len {
                self.shuffle_ptr += 1;
            } else if !auto_finished || self.config.repeat == RepeatMode::All {
                self.rebuild_shuffle_order();
                self.shuffle_ptr = 0;
            } else {
                self.is_playing = false;
                return;
            }
            if let Some(&next_idx) = self.shuffle_order.get(self.shuffle_ptr) {
                if let Some(tr) = self.active_pool.get(next_idx).cloned() {
                    self.current_pool_idx = Some(next_idx);
                    self.play_track_direct(tr, true, use_crossfade, ctx);
                }
            }
        } else {
            let cur = self.current_pool_idx.unwrap_or(0);
            if cur + 1 < len {
                let next_idx = cur + 1;
                if let Some(tr) = self.active_pool.get(next_idx).cloned() {
                    self.current_pool_idx = Some(next_idx);
                    self.play_track_direct(tr, true, use_crossfade, ctx);
                }
            } else if !auto_finished || self.config.repeat == RepeatMode::All {
                if let Some(tr) = self.active_pool.first().cloned() {
                    self.current_pool_idx = Some(0);
                    self.play_track_direct(tr, true, use_crossfade, ctx);
                }
            } else {
                self.is_playing = false;
            }
        }
    }
    pub fn step_prev_or_restart(&mut self, ctx: &egui::Context) {
        if self.current_pos > 3.5 {
            self.current_pos = 0.0;
            let _ = self.audio_tx.send(AudioCmd::Seek(0.0));
            return;
        }
        if self.playback_ctx == PlaybackContext::Wave {
            if let Some(prev_hist) = self.history.pop_front() {
                if let Some(cur) = self.current_track.take() { self.wave_buffer.push_front(cur); }
                self.play_track_direct(prev_hist, false, false, ctx);
            } else {
                self.current_pos = 0.0;
                let _ = self.audio_tx.send(AudioCmd::Seek(0.0));
            }
            return;
        }
        let len = self.active_pool.len();
        if len == 0 {
            if let Some(prev_hist) = self.history.pop_front() {
                self.play_track_direct(prev_hist, false, false, ctx);
            }
            return;
        }
        if self.config.shuffle && self.shuffle_order.len() == len {
            self.shuffle_ptr = if self.shuffle_ptr > 0 { self.shuffle_ptr - 1 } else { len - 1 };
            if let Some(&idx) = self.shuffle_order.get(self.shuffle_ptr) {
                if let Some(tr) = self.active_pool.get(idx).cloned() {
                    self.current_pool_idx = Some(idx);
                    self.play_track_direct(tr, false, false, ctx);
                }
            }
        } else {
            let cur = self.current_pool_idx.unwrap_or(0);
            let prev_idx = if cur == 0 { len - 1 } else { cur - 1 };
            if let Some(tr) = self.active_pool.get(prev_idx).cloned() {
                self.current_pool_idx = Some(prev_idx);
                self.play_track_direct(tr, false, false, ctx);
            }
        }
    }

    pub fn toggle_current_like(&mut self) {
        let Some(tr) = &mut self.current_track else { return };
        if tr.provider != TrackProvider::Yandex || tr.local_path.is_some() { return; }
        tr.liked = !tr.liked;
        let new_state = tr.liked;
        let tid = tr.id.clone();

        if new_state {
            if !self.liked_ids.contains(&tid) { self.liked_ids.insert(0, tid.clone()); }
        } else {
            self.liked_ids.retain(|x| x != &tid);
        }
        for list in [&mut self.playlist_tracks, &mut self.active_pool, &mut self.search_tracks, &mut self.last_search_tracks] {
            for item in list.iter_mut() { if item.id == tid { item.liked = new_state; } }
        }
        for q in [&mut self.user_queue, &mut self.wave_buffer, &mut self.history] {
            for item in q.iter_mut() { if item.id == tid { item.liked = new_state; } }
        }
        if let Some(uid) = self.ym_uid {
            let tok = self.config.token.clone();
            thread::spawn(move || { set_track_like_remote(&tok, uid, &tid, new_state); });
        }
    }

    pub fn wake_from_sleep(&mut self, ctx: &egui::Context) {
        self.is_sleep_mode = false;
        set_process_eco_qos(false);
        force_wake_window_win32();
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(180.0, 140.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    pub fn toggle_sleep_mode(&mut self, ctx: &egui::Context) {
        capture_own_hwnd();
        if self.is_sleep_mode {
            self.wake_from_sleep(ctx);
        } else {
            force_hide_window_win32();
            self.is_sleep_mode = true;
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

    pub fn start_device_auth(&mut self, ctx: egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.auth_rx = Some(rx);
        self.auth_status = "Запрос кода авторизации...".into();
        self.device_code_text = None;
        spawn_device_auth(tx, ctx);
    }
    pub fn handle_window_resize(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) { return; }
        let rect = ctx.screen_rect();
        let border = 6.0;
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else { return };
        let (l, r, t, b) = (pos.x <= rect.left() + border, pos.x >= rect.right() - border, pos.y <= rect.top() + border, pos.y >= rect.bottom() - border);
        use egui::{CursorIcon, ResizeDirection};
        let dir = match (l, r, t, b) {
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

    pub fn render_custom_titlebar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("ostinato_titlebar")
            .exact_height(32.0)
            .show_separator_line(false)
            .frame(egui::Frame::none().fill(c_bg_bar()).inner_margin(egui::Margin::symmetric(12.0, 4.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let drag_w = (ui.available_width() - 115.0).max(50.0);
                    let (drag_rect, drag_resp) = ui.allocate_exact_size(Vec2::new(drag_w, 24.0), Sense::click_and_drag());
                    if drag_resp.double_clicked() {
                        let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                    } else if drag_resp.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    let mode_tag = match self.playback_ctx {
                        PlaybackContext::Wave => "МОЯ ВОЛНА",
                        PlaybackContext::Playlist => "БИБЛИОТЕКА",
                        PlaybackContext::Local => "ОФЛАЙН-ПАПКА",
                        PlaybackContext::SearchOrArtist => "ПОИСК",
                    };
                    let suffix = if self.sync_status.is_empty() { mode_tag.to_string() } else { self.sync_status.clone() };
                    ui.painter().text(drag_rect.left_center(), egui::Align2::LEFT_CENTER, format!("OSTINATO v4.0  //  {}", suffix), FontId::proportional(11.0), c_text_gray());

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if win_bar_btn(ui, 2).clicked() {
                            self._tray_icon.take();
                            std::process::exit(0);
                        }
                        if win_bar_btn(ui, 1).clicked() {
                            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                        }
                        if win_bar_btn(ui, 0).clicked() {
                            capture_own_hwnd();
                            minimize_window_win32();
                        }
                    });
                });
            });
    }
}

impl eframe::App for OstinatoApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        c_bg_void().to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme_init_id = egui::Id::new("os_theme_applied_v1");
        if !ctx.data(|d| d.get_temp::<bool>(theme_init_id).unwrap_or(false)) {
            setup_ostinato_theme(ctx);
            ctx.data_mut(|d| d.insert_temp(theme_init_id, true));
        }
        let is_minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let screen_sz = ctx.screen_rect().size();
        if !self.is_sleep_mode && !is_minimized && screen_sz.x > 50.0 && screen_sz.y > 50.0 {
            capture_own_hwnd();
        }

        while self.tray_wake_rx.try_recv().is_ok() { self.wake_from_sleep(ctx); }
        while let Ok(hk_id) = self.hk_rx.try_recv() {
            if let Some(&act) = self.hotkey_map.get(&hk_id) {
                match act {
                    BindAction::PlayPause => self.toggle_play_pause(ctx),
                    BindAction::Next => self.step_next(false, false, ctx),
                    BindAction::Prev => self.step_prev_or_restart(ctx),
                    BindAction::Like => self.toggle_current_like(),
                    BindAction::ShakeWave => self.launch_or_shake_wave(ctx),
                    BindAction::Sleep => self.toggle_sleep_mode(ctx),
                    BindAction::ToggleShuffle => self.toggle_shuffle(),
                    BindAction::CycleRepeat => self.cycle_repeat(),
                    BindAction::VolUp => self.adjust_volume_step(0.05),
                    BindAction::VolDown => self.adjust_volume_step(-0.05),
                }
            }
        }

        while let Ok(msg) = self.api_rx.try_recv() {
            match msg {
                ApiMsg::InitDone { uid, mut playlists, liked_ids, initial_tracks } => {
                    self.ym_uid = Some(uid);
                    for p in &mut playlists { p.title = format!("{} [YM]", p.title); }
                    self.ym_cloud_playlists = playlists;
                    self.rebuild_combined_playlists();
                    self.liked_ids = liked_ids;
                    self.playlist_tracks = initial_tracks.clone();
                    self.sync_status.clear();
                    if self.current_track.is_none() && !initial_tracks.is_empty() {
                        self.current_track = initial_tracks.first().cloned();
                        self.active_pool = initial_tracks;
                        self.current_pool_idx = Some(0);
                        self.rebuild_shuffle_order();
                    }
                }
                ApiMsg::LikesChunkAppended { tracks } => {
                    let is_likes = self.playlists.get(self.selected_playlist).map(|p| p.source == PlaylistSource::Likes).unwrap_or(false);
                    if is_likes {
                        for t in &tracks { if !self.playlist_tracks.iter().any(|x| x.id == t.id) { self.playlist_tracks.push(t.clone()); } }
                    }
                    if self.playback_ctx == PlaybackContext::Playlist && is_likes {
                        for t in tracks { if !self.active_pool.iter().any(|x| x.id == t.id) { self.active_pool.push(t); } }
                        self.rebuild_shuffle_order();
                    }
                }
                ApiMsg::PlaylistLoaded { tracks } => { self.playlist_tracks = tracks; self.sync_status.clear(); }
                ApiMsg::SearchDone { artists, albums, tracks } => {
                    self.search_artists = artists; self.search_albums = albums;
                    self.last_search_tracks = tracks.clone(); self.last_search_header = self.search_header.clone();
                    self.search_tracks = tracks; self.sync_status.clear();
                }
                ApiMsg::ArtistLoaded { artist_name, tracks } => { self.search_header = format!("Популярные треки: {}", artist_name); self.search_tracks = tracks; self.sync_status.clear(); }
                ApiMsg::AlbumLoaded { album_title, tracks } => { self.search_header = format!("Альбом: {}", album_title); self.search_tracks = tracks; self.sync_status.clear(); }
                ApiMsg::WaveBatchReady { tracks, batch_id, replace_and_play, step_next_immediately } => {
                    self.wave_loading = false; self.sync_status.clear();
                    if let Some(bid) = batch_id { self.wave_batch_id = Some(bid); }
                    if replace_and_play { self.wave_buffer.clear(); }
                    let mut fresh = 0;
                    for t in &tracks {
                        let is_cur = self.current_track.as_ref().map(|c| c.id == t.id).unwrap_or(false);
                        if !is_cur && !self.wave_seen_ids.contains(&t.id) && !self.wave_buffer.iter().any(|x| x.id == t.id) {
                            self.wave_buffer.push_back(t.clone()); fresh += 1;
                        }
                    }
                    if fresh == 0 {
                        for t in tracks {
                            if !self.current_track.as_ref().map(|c| c.id == t.id).unwrap_or(false) && !self.wave_buffer.iter().any(|x| x.id == t.id) {
                                self.wave_buffer.push_back(t);
                            }
                        }
                    }
                    if replace_and_play || step_next_immediately {
                        if let Some(first) = self.wave_buffer.pop_front() {
                            self.playback_ctx = PlaybackContext::Wave;
                            self.play_track_direct(first, true, false, ctx);
                        }
                    }
                }
                ApiMsg::AudioReady { seq, track, bytes, crossfade } => {
                    if seq == self.play_seq.load(Ordering::SeqCst) {
                        let dur = track.duration_sec;
                        self.sync_status.clear(); self.current_track = Some(track);
                        self.track_audio_loaded = true; self.is_playing = true;
                        self.sync_discord_presence();
                        let _ = self.audio_tx.send(AudioCmd::LoadBytes { seq, bytes, duration: dur, vol: self.config.volume, crossfade });
                        if self.is_sleep_mode { deep_trim_memory(); }
                    }
                }

                ApiMsg::PreloadReady { track_id, bytes } => { self.preloaded_audio = Some((track_id, bytes)); self.preloading_track_id = None; }
                ApiMsg::LyricsReady { track_id, synced, lines } => {
                    if self.lyrics_track_id.as_deref() == Some(track_id.as_str()) {
                        self.lyrics_loading = false; self.lyrics_synced = synced; self.lyrics_lines = lines;
                    }
                }
                ApiMsg::ExportDone(msg) => { self.sync_status = msg; self.refresh_local_folder(); }
                ApiMsg::Error(e) => { self.wave_loading = false; self.lyrics_loading = false; self.sync_status = format!("API: {}", e); }
                ApiMsg::VkAudioLoaded { .. } => {}

            }

        }
        let now_ui = ctx.input(|i| i.time);
        let is_dragging_seek = ctx.data(|d| d.get_temp::<bool>(egui::Id::new("seek_dragging")).unwrap_or(false));
        let seek_busy = is_dragging_seek;

        let fin_seq = self.audio_state.finished_seq.swap(0, Ordering::SeqCst);
        if fin_seq != 0 && fin_seq == self.play_seq.load(Ordering::SeqCst) && !seek_busy {
            self.step_next(true, false, ctx);
        }

        if self.is_playing {
            if !seek_busy {
                if let Ok(p) = self.audio_state.pos_sec.lock() { self.current_pos = *p; }
            }
            self.maybe_preload_next(ctx);
            if self.config.crossfade && !is_dragging_seek {
                if let Some(cur) = self.current_track.clone() {
                    let rem = cur.duration_sec - self.current_pos;
                    if cur.duration_sec > 15.0 && rem <= 2.4 && rem > 0.35 && self.crossfade_triggered_for.as_deref() != Some(cur.id.as_str()) {
                        self.crossfade_triggered_for = Some(cur.id);
                        self.step_next(true, true, ctx);
                    }
                }
            }
        }

        if self.is_sleep_mode {
            ctx.request_repaint_after(Duration::from_millis(500));
            std::thread::sleep(Duration::from_millis(45));
            return;
        }
        if is_minimized || screen_sz.x <= 10.0 || screen_sz.y <= 10.0 {
            egui::CentralPanel::default().frame(egui::Frame::none().fill(c_bg_void())).show(ctx, |_| {});
            return;
        }

        if self.is_playing && self.config.beat_reactive {
            let raw_b = self.audio_state.read_beat() * active_palette().beat_reactivity;
            let k = if raw_b > self.smooth_beat { 0.42 } else { 0.14 };
            self.smooth_beat += (raw_b - self.smooth_beat) * k;
            ctx.request_repaint_after(Duration::from_millis(28));
        } else {
            self.smooth_beat *= 0.82;
            if self.is_playing { ctx.request_repaint_after(Duration::from_millis(85)); }
        }

        let mut clear_auth = false;
        let mut trigger_ym = false;
        if let Some(rx) = self.auth_rx.take() {
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
                        self.config.save();
                        trigger_ym = true;
                        if self.screen == AppScreen::Onboarding {
                            self.screen = AppScreen::ReadyTransition(Instant::now());
                        }
                        self.device_code_text = None;
                        clear_auth = true;
                    }
                    AuthMsg::Error(err) => {
                        self.auth_status = err;
                        clear_auth = true;
                    }
                }
            }
            if !clear_auth {
                self.auth_rx = Some(rx);
            }
        }
        if trigger_ym { self.trigger_ym_init(ctx.clone()); }




        self.handle_window_resize(ctx);
        self.render_custom_titlebar(ctx);
        self.render_mix_modal(ctx);

        match self.screen {
            AppScreen::Onboarding => self.render_onboarding(ctx),
            AppScreen::ReadyTransition(t) => self.render_ready_transition(ctx, t),
            AppScreen::MainPlayer => self.render_main_player(ctx),
        }
    }
}
impl OstinatoApp {
    fn render_mix_modal(&mut self, ctx: &egui::Context) {
        let Some(track) = self.pending_mix_track.clone() else { return };
        let mut close = false;
        egui::Area::new(egui::Id::new("mix_add_modal"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(c_bg_panel())
                    .stroke(Stroke::new(1.5, c_neon()))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(18.0)
                    .show(ui, |ui| {
                        ui.set_width(320.0);
                        ui.label(RichText::new("ДОБАВИТЬ В МИКС-ПЛЕЙЛИСТ").size(12.5).color(c_bright()).strong());
                        ui.add_space(4.0);
                        let sub: String = format!("[{}] {} — {}", track.provider.short_tag(), track.artist, track.title).chars().take(42).collect();
                        ui.label(RichText::new(sub).size(11.0).color(c_text_gray()));
                        ui.add_space(10.0);

                        let mut chosen_id: Option<String> = None;
                        egui::ScrollArea::vertical().id_salt("mix_modal_sc").max_height(150.0).show(ui, |ui| {
                            for mx in &self.custom_playlists {
                                let lbl = format!("★ {} ({} тр.)", mx.title, mx.tracks.len());
                                if full_width_btn(ui, &lbl, 28.0, false) { chosen_id = Some(mx.id.clone()); }
                                ui.add_space(4.0);
                            }
                        });
                        if let Some(mid) = chosen_id {
                            self.add_track_to_mix(&mid, track.clone());
                            close = true;
                        }

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            dark_text_input(ui, 210.0, 20.0, &mut self.new_mix_name, "Новый микс-плейлист...", false, false);
                            if pill_action_btn(ui, "Создать", true) && !self.new_mix_name.trim().is_empty() {
                                let name = self.new_mix_name.clone();
                                self.new_mix_name.clear();
                                self.create_mix_playlist(&name);
                                if let Some(last) = self.custom_playlists.last().map(|m| m.id.clone()) {
                                    self.add_track_to_mix(&last, track.clone());
                                    close = true;
                                }
                            }
                        });
                        ui.add_space(8.0);
                        if full_width_btn(ui, "Отмена", 26.0, false) { close = true; }
                    });
            });
        if close { self.pending_mix_track = None; }
    }

    fn render_onboarding(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::none().fill(c_bg_void())).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                let top_pad = ((ui.available_height() - 370.0) * 0.45).max(24.0);
                ui.add_space(top_pad);
                ui.label(RichText::new("O S T I N A T O").font(FontId::proportional(34.0)).color(c_neon()).strong());
                ui.add_space(4.0);
                ui.label(RichText::new("v4.0  //  Мультиплатформенный аудиоклиент (YM · SC)").font(FontId::proportional(13.0)).color(c_text_gray()));
                    ui.add_space(22.0);

                egui::Frame::none().fill(c_bg_panel()).stroke(Stroke::new(1.0, c_border_dark())).rounding(Rounding::same(10.0)).inner_margin(24.0).show(ui, |ui| {
                    ui.set_width((ui.available_width() - 48.0).min(420.0));
                    ui.label(RichText::new("Быстрый старт").font(FontId::proportional(17.0)).color(c_bright()).strong());
                    ui.add_space(6.0);
                    ui.label(RichText::new("Подключите Яндекс по коду или сразу войдите в плеер — SoundCloud и локальные треки работают без аккаунта.").font(FontId::proportional(12.0)).color(c_text_gray()));
                    ui.add_space(16.0);

                    if let Some(code) = self.device_code_text.clone() {
                        let is_copied = self.copied_feedback_until.map(|u| Instant::now() < u).unwrap_or(false);
                        if is_copied { ctx.request_repaint_after(Duration::from_millis(100)); }
                        egui::Frame::none().fill(c_bg_void()).stroke(Stroke::new(1.5, if is_copied { c_bright() } else { c_neon() })).rounding(Rounding::same(8.0)).inner_margin(14.0).show(ui, |ui| {
                            ui.label(RichText::new(if is_copied { "Код скопирован!" } else { "Код подтверждения:" }).size(11.5).color(c_text_gray()));
                            ui.label(RichText::new(&code).font(FontId::proportional(30.0)).color(c_bright()).strong());
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if pill_action_btn(ui, if is_copied { "Скопировано ✓" } else { "Скопировать" }, is_copied) {
                                    ctx.copy_text(code.clone());
                                    self.copied_feedback_until = Some(Instant::now() + Duration::from_secs(2));
                                }
                                if let Some(url) = &self.device_verify_url {
                                    if pill_action_btn(ui, "Открыть сайт", true) { let _ = open::that(url); }
                                }
                            });
                        });
                    } else if !self.manual_mode {
                        if full_width_btn(ui, "Получить код Яндекс Музыки", 38.0, true) { self.start_device_auth(ctx.clone()); }
                    } else {
                        dark_text_input(ui, ui.available_width(), 22.0, &mut self.token_input, "Токен Яндекса y0_AgAAAA...", false, true);
                        ui.add_space(10.0);
                        if !self.token_input.trim().is_empty() && full_width_btn(ui, "Сохранить токен", 34.0, true) {
                            self.config.token = extract_token(&self.token_input);
                            self.config.save();
                            self.trigger_ym_init(ctx.clone());
                            self.screen = AppScreen::ReadyTransition(Instant::now());
                        }
                    }

                    ui.add_space(14.0);
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.columns(2, |cols| {
                        let m_txt = if self.manual_mode { "<- Вход по коду" } else { "Токен вручную" };
                        if let Some(c0) = cols.first_mut() {
                            if full_width_btn(c0, m_txt, 30.0, false) { self.manual_mode = !self.manual_mode; self.device_code_text = None; }
                        }
                        if let Some(c1) = cols.last_mut() {
                            if full_width_btn(c1, "SoundCloud / Офлайн", 30.0, false) {
                                self.search_provider = TrackProvider::SoundCloud;
                                self.active_tab = LeftTab::Search;
                                self.screen = AppScreen::ReadyTransition(Instant::now());
                            }
                        }
                    });
                });
            });
        });
    }
    fn render_ready_transition(&mut self, ctx: &egui::Context, start: Instant) {
        let el = start.elapsed().as_secs_f32();
        if el >= 1.4 { self.screen = AppScreen::MainPlayer; return; }
        let a = if el < 0.35 { el / 0.35 } else if el > 1.05 { (1.4 - el) / 0.35 } else { 1.0 }.clamp(0.0, 1.0);
        let pal = active_palette();
        let mc = ThemePalette::ca(pal.accent_bright, (a * 255.0) as u8);
        let sc = ThemePalette::ca(pal.text_gray, (a * 220.0) as u8);
        egui::CentralPanel::default().frame(egui::Frame::none().fill(c_bg_void())).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.38);
                ui.label(RichText::new("OSTINATO v4.0").font(FontId::proportional(38.0)).color(mc).strong());
                ui.add_space(8.0);
                ui.label(RichText::new("Мультиплатформенное ядро активировано").font(FontId::proportional(14.0)).color(sc));
            });
        });
        ctx.request_repaint();
    }
    fn render_fullscreen_karaoke(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, anim: f32) {
        let pal = active_palette();
        let full_r = ui.max_rect();
        let beat = if self.config.beat_reactive { self.smooth_beat } else { 0.0 };
        let alpha_k = anim.clamp(0.0, 1.0);
        let slide_y = (1.0 - alpha_k) * 24.0;

        if let Some(bg_g) = &self.skin_assets.bg_tex {
            let flash = if pal.beat_mode == 2 { beat * 0.12 } else { 0.0 };
            let a = ((pal.bg_image_alpha * 0.65 + flash).clamp(0.0, 1.0) * 255.0 * alpha_k) as u8;
            egui::Image::from_texture(bg_g.current_texture(ctx))
                .tint(Color32::from_white_alpha(a))
                .paint_at(ui, full_r);
        }
        ui.painter().rect_filled(
            full_r,
            Rounding::ZERO,
            ThemePalette::ca(pal.bg_void, (155.0 * alpha_k) as u8),
        );

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_lyrics = false;
        }

        ui.add_space(14.0 + slide_y);
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            ui.label(
                RichText::new("●  КАРАОКЕ")
                    .size(12.0)
                    .color(ThemePalette::ca(pal.accent_neon, (255.0 * alpha_k) as u8))
                    .strong(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(22.0);
                let (close_r, close_resp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
                if close_resp.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let bg = if close_resp.hovered() { c_bg_hover() } else { c_bg_widget() };
                let brd = if close_resp.hovered() { c_neon() } else { c_border_dark() };
                ui.painter().rect(
                    close_r,
                    Rounding::same(10.0),
                    bg.gamma_multiply(0.9 * alpha_k),
                    Stroke::new(1.2_f32, brd.gamma_multiply(alpha_k)),
                );

                let c = close_r.center();
                let st_col = if close_resp.hovered() { c_bright() } else { c_text_pri() };
                let st = Stroke::new(1.8_f32, st_col.gamma_multiply(alpha_k));
                ui.painter().line_segment([egui::pos2(c.x - 5.5, c.y - 5.5), egui::pos2(c.x + 5.5, c.y + 5.5)], st);
                ui.painter().line_segment([egui::pos2(c.x + 5.5, c.y - 5.5), egui::pos2(c.x - 5.5, c.y + 5.5)], st);

                if close_resp.clicked() {
                    self.show_lyrics = false;
                }
            });
        });

        let avail_w = ui.available_width();
        let avail_h = ui.available_height();
        let left_col_w = (avail_w * 0.36).clamp(300.0, 440.0);
        let right_col_w = (avail_w - left_col_w - 48.0).max(320.0);

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);

            ui.allocate_ui_with_layout(
                Vec2::new(left_col_w, avail_h - 24.0),
                egui::Layout::top_down(egui::Align::Center),

                |ui| {
                    let cover_sz = (left_col_w - 48.0).min(avail_h * 0.54).clamp(210.0, 360.0);
                    ui.add_space(((avail_h - cover_sz - 180.0) * 0.44).max(16.0));

                    let (slot_rect, _) = ui.allocate_exact_size(Vec2::splat(cover_sz), Sense::hover());
                    let rnd = Rounding::same(18.0);
                    let glow_k = (0.4 + beat * 1.15) * pal.glow_strength * alpha_k;
                    paint_neon_glow(ui.painter(), slot_rect, rnd, pal.accent_neon, glow_k);

                    if let Some(url) = self.current_track.as_ref().and_then(|t| t.cover_url.clone()) {
                        egui::Image::from_uri(url)
                            .rounding(rnd)
                            .tint(Color32::from_white_alpha((255.0 * alpha_k) as u8))
                            .paint_at(ui, slot_rect);
                        ui.painter().rect_stroke(
                            slot_rect,
                            rnd,
                            Stroke::new(1.5_f32, c_border_dark().gamma_multiply(alpha_k)),
                        );
                    } else {
                        ui.painter().rect(
                            slot_rect,
                            rnd,
                            c_bg_void().gamma_multiply(alpha_k),
                            Stroke::new(1.5_f32, c_border_dark().gamma_multiply(alpha_k)),
                        );
                    }

                    ui.add_space(18.0);
                    if let Some(tr) = &self.current_track {
                        let short_t: String = tr.title.chars().take(32).collect();
                        let short_a: String = tr.artist.chars().take(36).collect();
                        ui.label(
                            RichText::new(short_t)
                                .size(21.0)
                                .color(c_bright().gamma_multiply(alpha_k))
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(short_a)
                                .size(14.0)
                                .color(c_text_gray().gamma_multiply(alpha_k)),
                        );
                    }

                    ui.add_space(18.0);
                    let ic_map = self.skin_assets.icons.clone();
                    let ic = |k: &str| ic_map.get(k);
                    let liked = self.current_track.as_ref().map(|t| t.liked).unwrap_or(false);
                    let is_ym = self
                        .current_track
                        .as_ref()
                        .map(|t| t.provider == TrackProvider::Yandex && t.local_path.is_none())
                        .unwrap_or(false);

                    let (_, row_r) = ui.allocate_space(Vec2::new(200.0, 46.0));
                    let cy = row_r.center().y;
                    let mut cx = row_r.left() + 20.0;

                    if draw_media_icon_btn(ui, "fs_like", egui::Rect::from_center_size(egui::pos2(cx, cy), Vec2::splat(36.0)), MediaIcon::Heart(liked), false, !is_ym, ic("like")).clicked() {
                        self.toggle_current_like();
                    }
                    cx += 46.0;
                    if draw_media_icon_btn(ui, "fs_prev", egui::Rect::from_center_size(egui::pos2(cx, cy), Vec2::splat(38.0)), MediaIcon::Prev, false, false, ic("prev")).clicked() {
                        self.step_prev_or_restart(ctx);
                    }
                    cx += 50.0;
                    let (p_ic, p_key) = if self.is_playing { (MediaIcon::Pause, "pause") } else { (MediaIcon::Play, "play") };
                    if draw_media_icon_btn(ui, "fs_play", egui::Rect::from_center_size(egui::pos2(cx, cy), Vec2::splat(46.0)), p_ic, true, false, ic(p_key)).clicked() {
                        self.toggle_play_pause(ctx);
                    }
                    cx += 50.0;
                    if draw_media_icon_btn(ui, "fs_next", egui::Rect::from_center_size(egui::pos2(cx, cy), Vec2::splat(38.0)), MediaIcon::Next, false, false, ic("next")).clicked() {
                        self.step_next(false, false, ctx);
                    }
                },
            );
            ui.add_space(16.0);

            ui.allocate_ui_with_layout(
                Vec2::new(right_col_w, avail_h - 24.0),
                egui::Layout::top_down(egui::Align::Center),

                |ui| {
                    if self.lyrics_loading {
                        ui.add_space((avail_h - 60.0) * 0.45);
                        ui.label(RichText::new("Загрузка текста...").size(18.0).color(c_text_gray()).strong());
                        return;
                    }
                    if self.lyrics_lines.is_empty() {
                        ui.add_space((avail_h - 60.0) * 0.45);
                        ui.label(RichText::new("Текст для этого трека отсутствует").size(18.0).color(c_text_gray()).strong());
                        return;
                    }
                    if !self.lyrics_synced {
                        egui::ScrollArea::vertical().id_salt("fs_plain_lrc").show(ui, |ui| {
                            ui.add_space(24.0);
                            for ln in &self.lyrics_lines {
                                ui.vertical_centered(|ui| {
                                    ui.label(RichText::new(&ln.text).size(20.0).color(c_text_pri()).strong());
                                });
                                ui.add_space(12.0);
                            }
                        });
                        return;
                    }

                    ctx.request_repaint();

                    let cur_t = self.current_pos + 0.15;
                    let act_idx = self.lyrics_lines.iter().rposition(|l| l.time_sec <= cur_t);
                    let target_center = act_idx.map(|i| i as f32).unwrap_or(-1.0);

                    let smooth_id = egui::Id::new("karaoke_exp_smooth_y");
                    let dt = ctx.input(|i| i.stable_dt).min(0.05);
                    let prev_smooth = ctx.data_mut(|d| *d.get_temp_mut_or(smooth_id, target_center));
                    let smooth_center = if (target_center - prev_smooth).abs() > 4.0 {
                        target_center
                    } else {
                        let next_s = prev_smooth + (target_center - prev_smooth) * (dt * 12.5).min(1.0);
                        if (target_center - next_s).abs() < 0.001 { target_center } else { next_s }
                    };
                    ctx.data_mut(|d| d.insert_temp(smooth_id, smooth_center));

                    let base_row_idx = smooth_center.round() as isize;
                    let total_dur = self.current_track.as_ref().map(|t| t.duration_sec).unwrap_or(180.0);
                    let karaoke_font = egui::FontFamily::Proportional;

                    // Жёстко ставим центр текста в правую свободную зону (сильно правее обложки)
                    let text_zone_left = full_r.left() + left_col_w + 80.0;
                    let text_zone_right = full_r.right() - 48.0;
                    let center_x = (text_zone_left + text_zone_right) * 0.5;
                    let max_text_w = (text_zone_right - text_zone_left - 40.0).max(240.0);

                    let center_y = full_r.center().y;
                    let step_y = 72.0;
                    let mut seek_to: Option<f32> = None;

                    for offset in -2..=2_isize {
                        let target_i = base_row_idx + offset;
                        let rel_dist = (target_i as f32) - smooth_center;
                        let abs_dist = rel_dist.abs();
                        if abs_dist > 2.35 {
                            continue;
                        }

                        let focus = (1.0 - (abs_dist / 2.2).clamp(0.0, 1.0)).powf(1.55);
                        let font_sz = 16.5 + 17.5 * focus;
                        let base_alpha = (45.0 + 210.0 * focus) as u8;
                        let final_alpha = (base_alpha as f32 * alpha_k) as u8;
                        let row_y = center_y + rel_dist * step_y;

                        if target_i < 0 || (target_i as usize) >= self.lyrics_lines.len() {
                            if target_i == -1 && act_idx.is_none() {
                                let dots = match ((ctx.input(|i| i.time) * 2.5) as usize) % 4 {
                                    0 => "●  ·  ·",
                                    1 => "●  ●  ·",
                                    2 => "●  ●  ●",
                                    _ => "·  ·  ·",
                                };
                                ui.painter().text(
                                    egui::pos2(center_x, row_y),
                                    egui::Align2::CENTER_CENTER,
                                    dots,
                                    FontId::new(28.0, karaoke_font.clone()),
                                    ThemePalette::ca(pal.accent_neon, final_alpha),
                                );
                            }
                            continue;
                        }
                        let idx_u = target_i as usize;
                        let ln = &self.lyrics_lines[idx_u];
                        let is_current = act_idx == Some(idx_u);

                        let word_prog = if is_current {
                            let next_t = self.lyrics_lines.get(idx_u + 1).map(|n| n.time_sec).unwrap_or(total_dur);
                            let span = (next_t - ln.time_sec).max(0.35);
                            ((cur_t - ln.time_sec) / span).clamp(0.0, 1.0)
                        } else if (target_i as f32) < smooth_center {
                            1.0
                        } else {
                            0.0
                        };

                        let font_id = FontId::new(font_sz, karaoke_font.clone());
                        let mut job = egui::text::LayoutJob::default();
                        job.wrap.max_width = max_text_w;

                        let words: Vec<&str> = ln.text.split_whitespace().collect();
                        let lit_exact = words.len() as f32 * word_prog;

                        for (wi, w) in words.iter().enumerate() {
                            let word_col = if is_current {
                                if (wi as f32) < lit_exact {
                                    Color32::from_white_alpha(final_alpha)
                                } else {
                                    Color32::from_white_alpha((final_alpha as f32 * 0.42) as u8)
                                }
                            } else {
                                Color32::from_white_alpha(final_alpha)
                            };

                            let piece = if wi + 1 < words.len() { format!("{} ", w) } else { (*w).to_string() };
                            job.append(
                                &piece,
                                0.0,
                                egui::TextFormat {
                                    font_id: font_id.clone(),
                                    color: word_col,
                                    ..Default::default()
                                },
                            );
                        }

                        let galley = ui.fonts(|f| f.layout_job(job));
                        let tight_size = galley.rect.size();
                        let pad_rect = egui::Rect::from_center_size(
                            egui::pos2(center_x, row_y),
                            Vec2::new(tight_size.x + 28.0, tight_size.y + 14.0),
                        );

                        if is_current && focus > 0.55 {
                            ui.painter().rect_filled(
                                pad_rect,
                                Rounding::same(12.0),
                                ThemePalette::ca(pal.bg_void, (115.0 * focus * alpha_k) as u8),
                            );
                        }

                        let draw_pos = egui::pos2(
                            center_x - galley.rect.left() - tight_size.x * 0.5,
                            row_y - galley.rect.top() - tight_size.y * 0.5,
                        );

                        let bold_d = 0.55 + 0.55 * focus;
                        ui.painter().galley(egui::pos2(draw_pos.x + bold_d, draw_pos.y), galley.clone(), Color32::WHITE);
                        ui.painter().galley(egui::pos2(draw_pos.x, draw_pos.y + bold_d * 0.55), galley.clone(), Color32::WHITE);
                        ui.painter().galley(draw_pos, galley, Color32::WHITE);

                        let resp = ui.interact(pad_rect, ui.id().with(("fs_lrc_row", idx_u)), Sense::click());
                        if resp.hovered() {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            seek_to = Some(ln.time_sec);
                        }
                    }


                    if let Some(t) = seek_to {
                        self.current_pos = t;
                        let _ = self.audio_tx.send(AudioCmd::Seek(t));
                        self.sync_discord_presence();
                    }
                },
            );
        });
    }




    fn render_main_player(&mut self, ctx: &egui::Context) {
        let lrc_anim = ctx.animate_bool_with_time(
            egui::Id::new("fullscreen_karaoke_fade"),
            self.show_lyrics,
            0.28,
        );
        if lrc_anim > 0.005 {
            egui::Area::new(egui::Id::new("fullscreen_karaoke_overlay"))
                .order(egui::Order::Foreground)
                .fixed_pos(egui::pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    let screen_r = ctx.screen_rect();
                    ui.set_min_size(screen_r.size());
                    ui.set_max_size(screen_r.size());
                    ui.painter().rect_filled(
                        screen_r,
                        Rounding::ZERO,
                        c_bg_main().gamma_multiply(lrc_anim),
                    );
                    self.render_fullscreen_karaoke(ui, ctx, lrc_anim);
                });
            if self.show_lyrics {
                ctx.request_repaint();
                return;
            }
            ctx.request_repaint();
        }



        egui::SidePanel::left("os_left_nav")
            .resizable(false)
            .exact_width(56.0)
            .show_separator_line(false)
            .frame(egui::Frame::none().fill(c_bg_void()).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new("OS").font(FontId::proportional(17.0)).color(c_neon()).strong());
                    ui.add_space(14.0);
                    for (tab, tip) in [
                        (LeftTab::Wave, "Моя волна [YM]"),
                        (LeftTab::Playlists, "Библиотека и Микс-плейлисты"),
                        (LeftTab::Search, "Мульти-поиск (YM / SC )"),
                        (LeftTab::QueueHistory, "Очередь и История"),
                        (LeftTab::LocalFiles, "Локальная папка"),
                        (LeftTab::Accounts, "Аккаунты (Яндекс / SC )"),
                        (LeftTab::Themes, "Темы и Скины "),
                    ] {
                        if draw_side_nav_btn(ui, tab, self.active_tab == tab, tip) { self.active_tab = tab; }
                        ui.add_space(6.0);
                    }
                    let rem_h = (ui.available_height() - 42.0).max(4.0);
                    ui.add_space(rem_h);
                    if draw_side_nav_btn(ui, LeftTab::Settings, self.active_tab == LeftTab::Settings, "Настройки и горячие клавиши") {
                        self.active_tab = LeftTab::Settings;
                    }
                });
            });

        if self.active_tab == LeftTab::Search {
            egui::CentralPanel::default().frame(egui::Frame::none().fill(c_bg_main()).inner_margin(22.0)).show(ctx, |ui| {
                self.render_fullscreen_search(ui, ctx);
            });
            return;
        }

        egui::SidePanel::left("os_sub_panel")
            .resizable(true)
            .default_width(292.0)
            .width_range(245.0..=410.0)
            .show_separator_line(false)
            .frame(egui::Frame::none().fill(c_bg_panel()).inner_margin(14.0))
            .show(ctx, |ui| match self.active_tab {
                LeftTab::Wave => self.render_sub_wave(ui, ctx),
                LeftTab::Playlists => self.render_sub_playlists(ui, ctx),
                LeftTab::QueueHistory => self.render_sub_queue_history(ui, ctx),
                LeftTab::LocalFiles => self.render_sub_local(ui, ctx),
                LeftTab::Accounts => self.render_sub_accounts(ui, ctx),
                LeftTab::Themes => self.render_sub_themes(ui, ctx),
                LeftTab::Settings => self.render_sub_settings(ui, ctx),
                LeftTab::Search => {}
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(c_bg_main()))
            .show(ctx, |ui| {
                self.render_right_player(ui, ctx);
            });

    }
    fn render_fullscreen_search(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("ПОИСК").font(FontId::proportional(18.0)).color(c_bright()).strong());
            ui.add_space(12.0);
            for (prov, lbl) in [
                (TrackProvider::Yandex, "Яндекс [YM]"),
                (TrackProvider::SoundCloud, "SoundCloud [SC]"),
            ] {
                if wave_filter_chip(ui, lbl, self.search_provider == prov) {
                    self.search_provider = prov;
                    if !self.search_query.trim().is_empty() { self.trigger_search(ctx); }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pill_action_btn(ui, "Sleep", false) { self.toggle_sleep_mode(ctx); }
                if let Some(cur) = &self.current_track {
                    let b: String = format!("[{}] {} — {}", cur.provider.short_tag(), cur.artist, cur.title).chars().take(38).collect();
                    if pill_action_btn(ui, &b, true) { self.active_tab = LeftTab::Playlists; }
                }
            });
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let iw = (ui.available_width() - 115.0).max(150.0);
            let r = dark_text_input(ui, iw, 26.0, &mut self.search_query, "Название трека, ремикса или артиста (Enter)...", false, false);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) { self.trigger_search(ctx); }
            if pill_action_btn(ui, "Найти", true) { self.trigger_search(ctx); }
        });
        ui.add_space(14.0);

        if self.viewing_sublist {
            if pill_action_btn(ui, "<- Назад к поиску", false) {
                self.viewing_sublist = false;
                if !self.last_search_tracks.is_empty() {
                    self.search_tracks = self.last_search_tracks.clone();
                    self.search_header = self.last_search_header.clone();
                } else {
                    self.active_tab = LeftTab::Playlists;
                }
            }
            ui.add_space(10.0);
        } else if self.search_provider == TrackProvider::Yandex {
            if !self.search_artists.is_empty() {
                ui.label(RichText::new("АРТИСТЫ").size(11.0).color(c_text_gray()).strong());
                ui.add_space(4.0);
                let mut ch: Option<(String, String)> = None;
                ui.horizontal_wrapped(|ui| {
                    for a in &self.search_artists { if artist_chip_card(ui, &a.name) { ch = Some((a.id.clone(), a.name.clone())); } }
                });
                if let Some((id, nm)) = ch { self.open_artist(id, nm, ctx); }
                ui.add_space(10.0);
            }
            if !self.search_albums.is_empty() {
                ui.label(RichText::new("АЛЬБОМЫ").size(11.0).color(c_text_gray()).strong());
                ui.add_space(4.0);
                let mut ch: Option<(String, String)> = None;
                ui.horizontal_wrapped(|ui| {
                    for al in &self.search_albums { if album_chip_card(ui, &al.title, &al.artist, al.year) { ch = Some((al.id.clone(), al.title.clone())); } }
                });
                if let Some((id, tl)) = ch { self.open_album(id, tl, ctx); }
                ui.add_space(10.0);
            }
        }
        if !self.search_header.is_empty() {
            ui.label(RichText::new(&self.search_header).size(12.0).color(c_neon()).strong());
            ui.add_space(6.0);
        }

        let (mut ap, mut an, mut aq, mut am) = (None, None, None, None);
        let sc_out = egui::ScrollArea::vertical().id_salt("fs_search_sc").show(ui, |ui| {
            for (i, tr) in self.search_tracks.iter().enumerate() {
                let is_cur = self.current_track.as_ref().map(|c| c.id == tr.id).unwrap_or(false);
                let res = track_row_with_queue(ui, i, tr.provider, &tr.title, &tr.artist, tr.duration_sec, is_cur, true);
                if res.play { ap = Some(i); }
                if res.play_next { an = Some(tr.clone()); }
                if res.add_queue { aq = Some(tr.clone()); }
                if res.add_mix { am = Some(tr.clone()); }
                ui.add_space(4.0);
            }
        });
        paint_scroll_fades(ui.painter(), sc_out.inner_rect, active_palette().bg_main, 14.0);

        if let Some(idx) = ap { let p = self.search_tracks.clone(); self.start_track_from_pool(p, idx, PlaybackContext::SearchOrArtist, ctx); }
        if let Some(t) = an { self.enqueue_next(t); }
        if let Some(t) = aq { self.enqueue_last(t); }
        if let Some(t) = am { self.pending_mix_track = Some(t); }
    }

    fn render_sub_wave(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("МОЯ ВОЛНА [YM]").size(12.0).color(c_neon()).strong());
        ui.add_space(6.0);
        let wave_act = self.playback_ctx == PlaybackContext::Wave && self.current_track.is_some();
        if full_width_btn(ui, if wave_act { "Встряхнуть Мою волну" } else { "▶ Запустить Мою волну" }, 36.0, true) {
            self.launch_or_shake_wave(ctx);
        }
        ui.add_space(12.0);
        let mut ch = false;
        ui.label(RichText::new("ЭНЕРГИЯ").size(10.5).color(c_text_gray()).strong());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (v, l) in [("all", "Любая"), ("active", "Бодрая"), ("fun", "Весёлая"), ("calm", "Спокойная"), ("sad", "Грустная")] {
                if wave_filter_chip(ui, l, self.config.wave_settings.mood_energy == v) { self.config.wave_settings.mood_energy = v.into(); ch = true; }
            }
        });
        ui.add_space(10.0);
        ui.label(RichText::new("ХАРАКТЕР").size(10.5).color(c_text_gray()).strong());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (v, l) in [("default", "Обычный"), ("favorite", "Любимое"), ("discover", "Незнакомое"), ("popular", "Популярное")] {
                if wave_filter_chip(ui, l, self.config.wave_settings.diversity == v) { self.config.wave_settings.diversity = v.into(); ch = true; }
            }
        });
        if ch { self.config.save(); self.launch_or_shake_wave(ctx); }
    }
    fn render_sub_playlists(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("БИБЛИОТЕКА И МИКСЫ").size(11.5).color(c_neon()).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_text_btn(ui, "Обновить") {
                    self.trigger_ym_init(ctx.clone());
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let iw = (ui.available_width() - 62.0).max(90.0);
            dark_text_input(ui, iw, 18.0, &mut self.new_mix_name, "+ Новый микс...", false, false);
            if small_text_btn(ui, "Создать") && !self.new_mix_name.trim().is_empty() {
                let n = self.new_mix_name.clone();
                self.new_mix_name.clear();
                self.create_mix_playlist(&n);
            }
        });
        ui.add_space(6.0);

        let mut clicked_pl = None;
        egui::ScrollArea::vertical().id_salt("pl_sc").max_height(130.0).show(ui, |ui| {
            for (idx, pl) in self.playlists.iter().enumerate() {
                if list_item_row(ui, &pl.title, self.selected_playlist == idx) { clicked_pl = Some(idx); }
                ui.add_space(2.0);
            }
        });
        if let Some(idx) = clicked_pl { self.select_playlist(idx, ctx); }

        ui.add_space(6.0);
        if let Some(PlaylistSource::CustomMix(mid)) = self.playlists.get(self.selected_playlist).map(|p| p.source.clone()) {
            if small_text_btn(ui, "Удалить этот микс") {
                self.custom_playlists.retain(|m| m.id != mid);
                save_custom_playlists(&self.custom_playlists);
                self.rebuild_combined_playlists();
                self.playlist_tracks.clear();
            }
        }
        ui.painter().hline(ui.available_rect_before_wrap().x_range(), ui.cursor().top(), Stroke::new(1.0, c_border_dark()));
        ui.add_space(6.0);
        ui.label(RichText::new(format!("ТРЕКИ ({})", self.playlist_tracks.len())).size(11.0).color(c_text_gray()));
        ui.add_space(4.0);

        let (mut p_idx, mut n_tr, mut q_tr, mut m_tr) = (None, None, None, None);
        let sc = egui::ScrollArea::vertical().id_salt("tr_sc").show(ui, |ui| {
            for (i, tr) in self.playlist_tracks.iter().enumerate() {
                let is_c = self.current_track.as_ref().map(|t| t.id == tr.id).unwrap_or(false);
                let act = track_row_with_queue(ui, i, tr.provider, &tr.title, &tr.artist, tr.duration_sec, is_c, false);
                if act.play { p_idx = Some(i); }
                if act.play_next { n_tr = Some(tr.clone()); }
                if act.add_queue { q_tr = Some(tr.clone()); }
                if act.add_mix { m_tr = Some(tr.clone()); }
                ui.add_space(2.0);
            }
        });
        paint_scroll_fades(ui.painter(), sc.inner_rect, active_palette().bg_panel, 10.0);

        if let Some(idx) = p_idx { let pool = self.playlist_tracks.clone(); self.start_track_from_pool(pool, idx, PlaybackContext::Playlist, ctx); }
        if let Some(t) = n_tr { self.enqueue_next(t); }
        if let Some(t) = q_tr { self.enqueue_last(t); }
        if let Some(t) = m_tr { self.pending_mix_track = Some(t); }
    }

    fn render_sub_queue_history(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            if pill_action_btn(ui, &format!("Очередь ({})", self.user_queue.len()), !self.queue_subtab_history) { self.queue_subtab_history = false; }
            if pill_action_btn(ui, "История", self.queue_subtab_history) { self.queue_subtab_history = true; }
        });
        ui.add_space(10.0);
        if !self.queue_subtab_history {
            if self.user_queue.is_empty() {
                ui.label(RichText::new("Очередь пуста.\nНаведите на трек: >1, +Q или +M.").size(11.5).color(c_text_gray()));
            } else {
                if small_text_btn(ui, "Очистить") { self.user_queue.clear(); }
                let mut rm = None;
                egui::ScrollArea::vertical().id_salt("uq_sc").show(ui, |ui| {
                    for (i, tr) in self.user_queue.iter().enumerate() {
                        if list_item_row(ui, &format!("{}. [{}] {} — {}", i + 1, tr.provider.short_tag(), tr.artist, tr.title), false) { rm = Some(i); }
                    }
                });
                if let Some(i) = rm { if let Some(ch) = self.user_queue.remove(i) { self.play_track_direct(ch, true, false, ctx); } }
            }
        } else {
            let mut ch = None;
            egui::ScrollArea::vertical().id_salt("uh_sc").show(ui, |ui| {
                for tr in &self.history {
                    if list_item_row(ui, &format!("[{}] {} — {}", tr.provider.short_tag(), tr.artist, tr.title), false) { ch = Some(tr.clone()); }
                }
            });
            if let Some(t) = ch { self.play_track_direct(t, true, false, ctx); }
        }
    }

    fn render_sub_local(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("ЛОКАЛЬНАЯ ПАПКА").size(11.5).color(c_neon()).strong());
        ui.add_space(8.0);
        if full_width_btn(ui, "Выбрать папку с музыкой", 32.0, true) {
            if let Some(f) = rfd::FileDialog::new().pick_folder() {
                self.config.local_folder = Some(f);
                self.config.save();
                self.refresh_local_folder();
            }
        }
        ui.add_space(8.0);
        let (mut pi, mut nt, mut qt, mut mt) = (None, None, None, None);
        egui::ScrollArea::vertical().id_salt("loc_sc").show(ui, |ui| {
            for (i, tr) in self.local_tracks.iter().enumerate() {
                let ic = self.current_track.as_ref().map(|t| t.id == tr.id).unwrap_or(false);
                let a = track_row_with_queue(ui, i + 50_000, tr.provider, &tr.title, &tr.artist, tr.duration_sec, ic, false);
                if a.play { pi = Some(i); }
                if a.play_next { nt = Some(tr.clone()); }
                if a.add_queue { qt = Some(tr.clone()); }
                if a.add_mix { mt = Some(tr.clone()); }
                ui.add_space(2.0);
            }
        });
        if let Some(idx) = pi { let p = self.local_tracks.clone(); self.start_track_from_pool(p, idx, PlaybackContext::Local, ctx); }
        if let Some(t) = nt { self.enqueue_next(t); }
        if let Some(t) = qt { self.enqueue_last(t); }
        if let Some(t) = mt { self.pending_mix_track = Some(t); }
    }
    fn render_sub_accounts(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("АККАУНТЫ И ПЛОЩАДКИ").size(12.0).color(c_neon()).strong());
        ui.add_space(8.0);

        ui.label(RichText::new("1. ЯНДЕКС МУЗЫКА [YM]").size(11.0).color(c_bright()).strong());
        let ym_st = if self.ym_uid.is_some() { "Статус: Подключено ✓" } else if !self.config.token.is_empty() { "Статус: Токен сохранён" } else { "Статус: Не подключен" };
        ui.label(RichText::new(ym_st).size(10.5).color(c_text_gray()));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 36.0).max(60.0);
            if dark_text_input(ui, w, 18.0, &mut self.token_input, "Токен Яндекса", !self.show_token, true).changed() {
                self.config.token = extract_token(&self.token_input);
                self.config.save();
                self.trigger_ym_init(ctx.clone());
            }
            if eye_toggle_btn(ui, self.show_token) { self.show_token = !self.show_token; }
        });
        ui.add_space(4.0);
        if full_width_btn(ui, "Войти в Яндекс по коду устройства", 26.0, false) {
            self.device_code_text = None;
            self.manual_mode = false;
            self.screen = AppScreen::Onboarding;
        }

        ui.add_space(12.0);
        ui.painter().hline(ui.available_rect_before_wrap().x_range(), ui.cursor().top(), Stroke::new(1.0, c_border_dark()));
        ui.add_space(10.0);

        ui.label(RichText::new("2. SOUNDCLOUD [SC]").size(11.0).color(c_bright()).strong());
        ui.label(RichText::new("Работает без аккаунта во вкладке Поиск.\nПри блокировке в РФ включите VPN или Zapret.").size(10.5).color(c_text_gray()));
    }


    fn render_sub_themes(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("СТУДИЯ СКИНОВ (OSU! STYLE)").size(11.5).color(c_neon()).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_text_btn(ui, "Папка Skins") { let _ = open::that(skins_dir()); }
            });
        });
        ui.add_space(6.0);

        ui.columns(2, |cols| {
            if full_width_btn(&mut cols[0], "Импорт (.oskin/.zip)", 26.0, true) {
                if let Some(zp) = rfd::FileDialog::new().add_filter("Ostinato Skin", &["oskin", "zip"]).pick_file() {
                    match import_skin_zip(&zp) {
                        Ok(sk_name) => {
                            self.available_skins = list_available_skins();
                            self.apply_selected_theme(&sk_name, ctx);
                            self.sync_status = format!("Установлен {}", sk_name);
                        }
                        Err(e) => self.sync_status = e,
                    }
                }
            }
            if full_width_btn(&mut cols[1], "Поделиться (.oskin)", 26.0, false) {
                let def_name = format!("{}.oskin", self.new_skin_name.trim());
                if let Some(dest) = rfd::FileDialog::new().set_file_name(&def_name).add_filter("Ostinato Skin", &["oskin", "zip"]).save_file() {
                    let dir = self.ensure_editable_skin_dir();
                    let pal = active_palette();
                    match export_skin_zip(&dir, &pal, &dest) {
                        Ok(_) => self.sync_status = "Скин упакован в .oskin!".into(),
                        Err(e) => self.sync_status = format!("Ошибка экспорта: {}", e),
                    }
                }
            }
        });
        ui.add_space(6.0);

        let mut pick_skin: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("sk_list").max_height(88.0).show(ui, |ui| {
            for sk in &self.available_skins {
                if list_item_row(ui, sk, &self.config.active_theme == sk) { pick_skin = Some(sk.clone()); }
            }
        });
        if let Some(s) = pick_skin { self.apply_selected_theme(&s, ctx); }
        ui.add_space(6.0);

        let mut pal = active_palette();
        let mut edited = false;

        egui::ScrollArea::vertical().id_salt("sk_studio_sc").show(ui, |ui| {
            ui.label(RichText::new("1. ФОН И АВТО-ЦВЕТА").size(10.5).color(c_bright()).strong());
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                if pill_action_btn(ui, "Фон...", true) {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Images / GIF", &["png", "jpg", "jpeg", "webp", "gif"]).pick_file() {
                        let dir = self.ensure_editable_skin_dir();
                        for old in ["bg.gif", "bg.png", "bg.jpg", "bg.webp", "background.gif", "background.png", "background.jpg"] {
                            let _ = std::fs::remove_file(dir.join(old));
                        }
                        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
                        let _ = std::fs::copy(&f, dir.join(format!("bg.{}", ext)));
                        self.reload_current_skin_assets(ctx);
                    }
                }
                if pill_action_btn(ui, "Авто-цвета", false) {
                    if let Some(bp) = self.skin_assets.bg_path.clone() {
                        match auto_palette_from_image(&bp, pal) {
                            Ok(np) => { pal = np; edited = true; self.sync_status = "Палитра подобрана под фон!".into(); }
                            Err(e) => self.sync_status = e,
                        }
                    } else {
                        self.sync_status = "Сначала выберите картинку фона".into();
                    }
                }
                if small_text_btn(ui, "Убрать") {
                    let dir = active_skin_folder(&self.config.active_theme);
                    for old in ["bg.gif", "bg.png", "bg.jpg", "bg.webp", "background.gif", "background.png", "background.jpg"] {
                        let _ = std::fs::remove_file(dir.join(old));
                    }
                    self.reload_current_skin_assets(ctx);
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Яркость фона:").size(11.0).color(c_text_gray()));
                edited |= ui.add(egui::Slider::new(&mut pal.bg_image_alpha, 0.05..=0.85).show_value(false)).changed();
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Матовое стекло:").size(11.0).color(c_text_gray()));
                edited |= ui.add(egui::Slider::new(&mut pal.glass_alpha, 0.35..=1.0).show_value(false)).changed();
            });
            ui.add_space(6.0);
            ui.label(RichText::new("2. МАСКОТ (ПЕРСОНАЖ)").size(10.5).color(c_bright()).strong());
            ui.horizontal(|ui| {
                if pill_action_btn(ui, "Маскот...", true) {
                    if let Some(f) = rfd::FileDialog::new().add_filter("PNG/WebP/GIF", &["png", "webp", "gif"]).pick_file() {
                        let dir = self.ensure_editable_skin_dir();
                        for old in ["mascot.gif", "mascot.png", "mascot.webp", "character.gif", "character.png", "anime.gif", "anime.png"] {
                            let _ = std::fs::remove_file(dir.join(old));
                        }
                        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
                        let _ = std::fs::copy(&f, dir.join(format!("mascot.{}", ext)));
                        self.reload_current_skin_assets(ctx);
                    }
                }
                if pill_action_btn(ui, if pal.mascot_left { "Слева" } else { "Справа" }, false) { pal.mascot_left = !pal.mascot_left; edited = true; }
                if small_text_btn(ui, "Убрать") {
                    let dir = active_skin_folder(&self.config.active_theme);
                    for old in ["mascot.gif", "mascot.png", "mascot.webp", "character.gif", "character.png", "anime.gif", "anime.png"] {
                        let _ = std::fs::remove_file(dir.join(old));
                    }
                    self.reload_current_skin_assets(ctx);
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Прозрачн.:").size(11.0).color(c_text_gray()));
                edited |= ui.add(egui::Slider::new(&mut pal.mascot_alpha, 0.15..=1.0).show_value(false)).changed();
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Размер:").size(11.0).color(c_text_gray()));
                edited |= ui.add(egui::Slider::new(&mut pal.mascot_scale, 0.5..=1.6).show_value(false)).changed();
            });
            if full_width_btn(ui, if pal.mascot_bounce { "Качание маскота в бит: ВКЛ" } else { "Качание маскота в бит: ВЫКЛ" }, 23.0, pal.mascot_bounce) {
                pal.mascot_bounce = !pal.mascot_bounce; edited = true;
            }

            ui.add_space(6.0);
            ui.label(RichText::new("3. ШРИФТ И ИКОНКИ КНОПОК").size(10.5).color(c_bright()).strong());
            ui.horizontal(|ui| {
                if pill_action_btn(ui, "Шрифт (.ttf/.otf)", true) {
                    if let Some(f) = rfd::FileDialog::new().add_filter("Font", &["ttf", "otf"]).pick_file() {
                        let dir = self.ensure_editable_skin_dir();
                        let _ = std::fs::remove_file(dir.join("font.ttf"));
                        let _ = std::fs::remove_file(dir.join("font.otf"));
                        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("ttf").to_ascii_lowercase();
                        let _ = std::fs::copy(&f, dir.join(format!("font.{}", ext)));
                        self.reload_current_skin_assets(ctx);
                    }
                }
                if small_text_btn(ui, "Сброс шрифта") {
                    let dir = active_skin_folder(&self.config.active_theme);
                    let _ = std::fs::remove_file(dir.join("font.ttf"));
                    let _ = std::fs::remove_file(dir.join("font.otf"));
                    self.reload_current_skin_assets(ctx);
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for &(key, ru) in &[
                    ("play", "Play"), ("pause", "Pause"), ("prev", "Prev"), ("next", "Next"),
                    ("like", "Like"), ("shuffle", "Shuf"), ("repeat", "Rep"), ("lyrics", "Lrc"),
                ] {
                    let has = self.skin_assets.icons.contains_key(key);
                    if wave_filter_chip(ui, ru, has) {
                        if let Some(f) = rfd::FileDialog::new().add_filter("Icon (PNG/GIF)", &["png", "gif"]).pick_file() {
                            let dir = self.ensure_editable_skin_dir();
                            let _ = std::fs::remove_file(dir.join(format!("{}.png", key)));
                            let _ = std::fs::remove_file(dir.join(format!("{}.gif", key)));
                            let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
                            let _ = std::fs::copy(&f, dir.join(format!("{}.{}", key, ext)));
                            self.reload_current_skin_assets(ctx);
                        }
                    }
                }
            });
            if !self.skin_assets.icons.is_empty() && small_text_btn(ui, "Сбросить все иконки кнопок") {
                let dir = active_skin_folder(&self.config.active_theme);
                for &k in SKIN_ICON_KEYS {
                    let _ = std::fs::remove_file(dir.join(format!("{}.png", k)));
                    let _ = std::fs::remove_file(dir.join(format!("{}.gif", k)));
                }
                self.reload_current_skin_assets(ctx);
            }

            ui.add_space(6.0);
            ui.label(RichText::new("4. РЕАКЦИЯ НА БИТ И ЦВЕТА").size(10.5).color(c_bright()).strong());
            if full_width_btn(ui, if self.config.beat_reactive { "Bass Glow: ВКЛ" } else { "Bass Glow: ВЫКЛ" }, 24.0, self.config.beat_reactive) {
                self.config.beat_reactive = !self.config.beat_reactive; self.config.save();
            }
            ui.horizontal_wrapped(|ui| {
                for (m, lbl) in [(0u8, "Дыхание"), (1, "Спектр"), (2, "Вспышка фона")] {
                    if wave_filter_chip(ui, lbl, pal.beat_mode == m) { pal.beat_mode = m; edited = true; }
                }
            });
            for (lbl, rgb) in [
                ("Неоновый акцент", &mut pal.accent_neon), ("Яркий текст", &mut pal.accent_bright),
                ("Глубокий акцент", &mut pal.accent_deep), ("Фон окна", &mut pal.bg_main), ("Фон панелей", &mut pal.bg_panel),
            ] {
                let mut arr = [rgb.0, rgb.1, rgb.2];
                ui.horizontal(|ui| {
                    if ui.color_edit_button_srgb(&mut arr).changed() { *rgb = (arr[0], arr[1], arr[2]); edited = true; }
                    ui.label(RichText::new(lbl).size(11.0).color(c_text_pri()));
                });
            }
            ui.horizontal(|ui| { ui.label(RichText::new("Glow:").size(11.0).color(c_text_gray())); edited |= ui.add(egui::Slider::new(&mut pal.glow_strength, 0.0..=2.0).show_value(false)).changed(); });
            ui.horizontal(|ui| { ui.label(RichText::new("Bass:").size(11.0).color(c_text_gray())); edited |= ui.add(egui::Slider::new(&mut pal.beat_reactivity, 0.0..=2.0).show_value(false)).changed(); });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                dark_text_input(ui, (ui.available_width() - 90.0).max(70.0), 18.0, &mut self.new_skin_name, "Имя нового скина...", false, false);
                if small_text_btn(ui, "Сохранить как") {
                    if let Ok(dir) = save_skin_folder(&self.new_skin_name, &pal) {
                        if let Some(n) = dir.file_name().and_then(|s| s.to_str()) {
                            self.available_skins = list_available_skins();
                            self.apply_selected_theme(&format!("Skin: {}", n), ctx);
                        }
                    }
                }
            });
        });

        if edited {
            self.config.custom_palette = pal;
            set_active_palette(pal);
            setup_ostinato_theme(ctx);
            if self.config.active_theme.starts_with("Skin: ") {
                let dir = active_skin_folder(&self.config.active_theme);
                if let Ok(json) = serde_json::to_string_pretty(&pal) { let _ = std::fs::write(dir.join("skin.json"), json); }
            } else {
                self.config.active_theme = "Custom".into();
            }
            self.config.save();
        }
    }


    fn render_sub_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("НАСТРОЙКИ И ХОТКЕИ").size(11.5).color(c_neon()).strong());
        ui.add_space(6.0);
        let cf_lbl = if self.config.crossfade { "Crossfade (2.4 сек): ВКЛЮЧЕН" } else { "Crossfade (2.4 сек): ВЫКЛЮЧЕН" };
        if full_width_btn(ui, cf_lbl, 26.0, self.config.crossfade) {
            self.config.crossfade = !self.config.crossfade;
            self.config.save();
        }
        ui.add_space(10.0);
        ui.label(RichText::new("ГОРЯЧИЕ КЛАВИШИ (ПОВЕРХ ИГР)").size(11.0).color(c_neon()).strong());
        ui.add_space(6.0);

        if let Some(action_id) = self.rebinding_action.clone() {
            match capture_key_combo(ctx) {
                KeyCapture::Combo(combo) => {
                    for (sid, sval) in [
                        ("play_pause", &mut self.config.keybinds.play_pause), ("next", &mut self.config.keybinds.next_track),
                        ("prev", &mut self.config.keybinds.prev_track), ("like", &mut self.config.keybinds.like_track),
                        ("wave", &mut self.config.keybinds.shake_wave), ("sleep", &mut self.config.keybinds.sleep_mode),
                        ("shuffle", &mut self.config.keybinds.toggle_shuffle), ("repeat", &mut self.config.keybinds.cycle_repeat),
                        ("vol_up", &mut self.config.keybinds.vol_up), ("vol_down", &mut self.config.keybinds.vol_down),
                    ] {
                        if sid == action_id.as_str() { *sval = combo.clone(); }
                        else if *sval == combo { sval.clear(); }
                    }
                    self.rebinding_action = None; self.config.save(); self.reload_global_hotkeys();
                }
                KeyCapture::Cleared => {
                    for (sid, sval) in [
                        ("play_pause", &mut self.config.keybinds.play_pause), ("next", &mut self.config.keybinds.next_track),
                        ("prev", &mut self.config.keybinds.prev_track), ("like", &mut self.config.keybinds.like_track),
                        ("wave", &mut self.config.keybinds.shake_wave), ("sleep", &mut self.config.keybinds.sleep_mode),
                        ("shuffle", &mut self.config.keybinds.toggle_shuffle), ("repeat", &mut self.config.keybinds.cycle_repeat),
                        ("vol_up", &mut self.config.keybinds.vol_up), ("vol_down", &mut self.config.keybinds.vol_down),
                    ] { if sid == action_id.as_str() { sval.clear(); } }
                    self.rebinding_action = None; self.config.save(); self.reload_global_hotkeys();
                }
                KeyCapture::Cancelled => self.rebinding_action = None,
                KeyCapture::None => {}
            }
        }

        egui::ScrollArea::vertical().id_salt("binds_sc").show(ui, |ui| {
            for (lbl, id, val) in [
                ("Пауза / Воспроизведение", "play_pause", self.config.keybinds.play_pause.clone()),
                ("Следующий трек", "next", self.config.keybinds.next_track.clone()),
                ("Предыдущий / В начало", "prev", self.config.keybinds.prev_track.clone()),
                ("Громкость +5%", "vol_up", self.config.keybinds.vol_up.clone()),
                ("Громкость -5%", "vol_down", self.config.keybinds.vol_down.clone()),
                ("Мне нравится (Лайк)", "like", self.config.keybinds.like_track.clone()),
                ("Встряхнуть Мою волну", "wave", self.config.keybinds.shake_wave.clone()),
                ("Sleep (Скрыть в трей)", "sleep", self.config.keybinds.sleep_mode.clone()),
                ("Перемешивание (Shuffle)", "shuffle", self.config.keybinds.toggle_shuffle.clone()),
                ("Повтор (Repeat)", "repeat", self.config.keybinds.cycle_repeat.clone()),
            ] {
                Self::bind_row(ui, lbl, id, &val, &mut self.rebinding_action);
            }
        });
    }

    fn bind_row(ui: &mut egui::Ui, label: &str, id: &str, cur: &str, rebinding: &mut Option<String>) {
        ui.label(RichText::new(label).size(11.0).color(c_text_gray()));
        let wait = rebinding.as_deref() == Some(id);
        let txt = if wait { "Нажмите клавишу (Del — сброс, Esc — отмена)..." } else if cur.trim().is_empty() { "Не назначено" } else { cur };
        if full_width_btn(ui, txt, 25.0, wait) { *rebinding = if wait { None } else { Some(id.to_string()) }; }
        ui.add_space(4.0);
    }

    fn render_right_player(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let pal = active_palette();
        let full_r = ui.max_rect();
        let beat = if self.config.beat_reactive { self.smooth_beat } else { 0.0 };

        if let Some(bg_g) = &self.skin_assets.bg_tex {
            let flash = if pal.beat_mode == 2 { beat * 0.16 } else { 0.0 };
            let a = ((pal.bg_image_alpha + flash).clamp(0.0, 1.0) * 255.0) as u8;
            egui::Image::from_texture(bg_g.current_texture(ctx)).tint(Color32::from_white_alpha(a)).paint_at(ui, full_r);
        }
        if let Some(m_g) = &self.skin_assets.mascot_tex {
            let mh = (full_r.height() * 0.56 * pal.mascot_scale).clamp(110.0, 520.0);
            let mw = mh * self.skin_assets.mascot_aspect.clamp(0.35, 1.8);
            let bounce_y = if pal.mascot_bounce { beat * 5.5 } else { 0.0 };
            let min_x = if pal.mascot_left { full_r.left() + 6.0 } else { full_r.right() - mw - 6.0 };
            let mr = egui::Rect::from_min_size(egui::pos2(min_x, full_r.bottom() - mh - bounce_y), Vec2::new(mw, mh));
            let tint = Color32::from_white_alpha((pal.mascot_alpha.clamp(0.0, 1.0) * 255.0) as u8);
            egui::Image::from_texture(m_g.current_texture(ctx)).tint(tint).paint_at(ui, mr);
        }


        let avail_w = ui.available_width();
        let avail_h = ui.available_height();
        let scale = (avail_h / 540.0).min(avail_w / 620.0).clamp(0.92, 1.85);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            if pill_action_btn(ui, if self.playback_ctx == PlaybackContext::Wave { "Встряхнуть волну" } else { "Моя волна" }, self.playback_ctx == PlaybackContext::Wave) {
                self.launch_or_shake_wave(ctx);
            }
            if let Some(cur) = &self.current_track {
                if pill_action_btn(ui, "+ В микс", false) { self.pending_mix_track = Some(cur.clone()); }
            }
            if !self.user_queue.is_empty() && pill_action_btn(ui, &format!("Очередь: {}", self.user_queue.len()), false) {
                self.active_tab = LeftTab::QueueHistory; self.queue_subtab_history = false;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                if pill_action_btn(ui, "Sleep", true) { self.toggle_sleep_mode(ctx); }
            });
        });


        let cover_side = (avail_h * 0.42).min(avail_w * 0.55).clamp(190.0, 410.0);
        let title_sz = (21.0 * scale).clamp(19.0, 34.0);
        let artist_sz = (13.5 * scale).clamp(13.0, 20.0);
        let btn_sm = (36.0 * scale).clamp(34.0, 50.0);
        let btn_lg = (46.0 * scale).clamp(42.0, 64.0);
        let gap_y = (17.0 * scale).clamp(11.0, 30.0);

        let content_h = cover_side + gap_y * 2.7 + title_sz + artist_sz + 28.0 + btn_lg;
        ui.add_space(((avail_h - 36.0 - content_h) * 0.45).max(8.0));

        ui.vertical_centered(|ui| {
            let cover_size = Vec2::splat(cover_side);
            let rounding = Rounding::same((16.0 * scale).clamp(14.0, 26.0));

if false {
} else {

                let (slot_rect, resp) = ui.allocate_exact_size(cover_size, Sense::click());
                let breath = beat * 2.4 * scale;
                let draw_r = slot_rect.expand(breath);
                let glow_k = (0.32 + beat * 1.25) * pal.glow_strength;
                paint_neon_glow(ui.painter(), draw_r, rounding, pal.accent_neon, glow_k);

                if pal.beat_mode == 1 && self.is_playing {
                    let t_now = ctx.input(|i| i.time) as f32;
                    for b_i in 0..12 {
                        let ratio = (b_i as f32 + 0.5) / 12.0;
                        let bx = draw_r.left() + draw_r.width() * ratio;
                        let bh = ((t_now * 6.5 + b_i as f32 * 0.8).sin().abs() * 8.0 + beat * 26.0) * scale;
                        let eq_r = egui::Rect::from_min_max(
                            egui::pos2(bx - 2.5 * scale, draw_r.top() - bh - 3.0),
                            egui::pos2(bx + 2.5 * scale, draw_r.top() - 2.0),
                        );
                        ui.painter().rect_filled(eq_r, Rounding::same(2.0), c_neon());
                    }
                }

                if let Some(url) = self.current_track.as_ref().and_then(|t| t.cover_url.clone()) {
                    egui::Image::from_uri(url).rounding(rounding).paint_at(ui, draw_r);
                    ui.painter().rect_stroke(draw_r, rounding, Stroke::new(1.5, if beat > 0.28 { c_neon() } else { c_border_dark() }));
                } else {
                    ui.painter().rect(draw_r, rounding, c_bg_void(), Stroke::new(1.5, c_border_dark()));
                    let c = draw_r.center();
                    let t_now = ctx.input(|i| i.time) as f32;
                    for i in 0..9 {
                        let off = (i as f32 - 4.0) * 11.0 * scale;
                        let wave = if self.is_playing { ((t_now * 5.0 + i as f32 * 0.9).sin().abs() * 14.0 + beat * 38.0) * scale } else { 4.0 };
                        let h = (10.0 * scale + wave).clamp(6.0, cover_side * 0.65);
                        let br = egui::Rect::from_center_size(egui::pos2(c.x + off, c.y), Vec2::new(5.5 * scale, h));
                        ui.painter().rect_filled(br, Rounding::same(2.8), if beat > 0.3 { c_neon() } else { c_deep() });
                    }
                }
                if resp.hovered() { ctx.set_cursor_icon(egui::CursorIcon::PointingHand); }
                show_nl_tooltip(&resp, "Нажмите, чтобы открыть текст трека");
                if resp.clicked() && self.current_track.is_some() {
                    self.show_lyrics = true;
                    self.request_lyrics_for_current(ctx);
                }
            }
            ui.add_space(gap_y * 0.85);
            let (prov, title, artist, artist_id, duration, liked) = if let Some(t) = &self.current_track {
                (t.provider, t.title.clone(), t.artist.clone(), t.artist_id.clone(), t.duration_sec.max(1.0), t.liked)
            } else {
                (TrackProvider::Yandex, "Выберите трек".into(), "Ostinato v4.0".into(), None, 100.0, false)
            };

            let max_ch = ((avail_w * 0.82) / (title_sz * 0.52)).max(26.0) as usize;
            let short_t: String = title.chars().take(max_ch).collect();
            let short_a: String = format!("[{}]  {}", prov.short_tag(), artist).chars().take(max_ch + 8).collect();

            ui.label(RichText::new(short_t).font(FontId::proportional(title_sz)).color(c_bright()).strong());
            ui.add_space(4.0);
            if let Some(aid) = artist_id {
                let tw = (short_a.chars().count() as f32 * (artist_sz * 0.56) + 16.0).clamp(60.0, avail_w * 0.75);
                let (ar, aresp) = ui.allocate_exact_size(Vec2::new(tw, artist_sz + 8.0), Sense::click());
                if aresp.hovered() { ctx.set_cursor_icon(egui::CursorIcon::PointingHand); }
                ui.painter().text(ar.center(), egui::Align2::CENTER_CENTER, &short_a, FontId::proportional(artist_sz), if aresp.hovered() { c_bright() } else { c_text_gray() });
                if aresp.clicked() { self.open_artist(aid, artist, ctx); }
            } else {
                ui.label(RichText::new(short_a).font(FontId::proportional(artist_sz)).color(c_text_gray()));
            }
            ui.add_space(gap_y);

            let bar_w = (avail_w * 0.74).clamp(210.0, 760.0);
            let (_, row_rect) = ui.allocate_space(Vec2::new(bar_w + 96.0, 22.0));
            let left_t = egui::Rect::from_min_size(row_rect.min, Vec2::new(42.0, 22.0));
            let right_t = egui::Rect::from_min_size(egui::pos2(row_rect.max.x - 42.0, row_rect.min.y), Vec2::new(42.0, 22.0));
            let seek_rect = egui::Rect::from_center_size(row_rect.center(), Vec2::new(bar_w, 18.0));
            let tf = (11.5 * scale.sqrt()).clamp(11.5, 14.0);
            ui.painter().text(left_t.left_center(), egui::Align2::LEFT_CENTER, format_time(self.current_pos), FontId::proportional(tf), c_text_gray());
            ui.painter().text(right_t.right_center(), egui::Align2::RIGHT_CENTER, format_time(duration), FontId::proportional(tf), c_text_gray());

            let seek_resp = ui.interact(seek_rect, ui.id().with("os_seek_bar"), Sense::click_and_drag());
            let dragging_now = seek_resp.dragged();
            let max_seek = (duration - 1.2).max(0.0);

            if dragging_now && seek_rect.width() > 0.0 {
                if let Some(p) = seek_resp.interact_pointer_pos() {
                    self.current_pos = (((p.x - seek_rect.left()) / seek_rect.width()).clamp(0.0, 1.0) * duration).min(max_seek);
                }
            }
            if (seek_resp.clicked() || seek_resp.drag_stopped()) && seek_rect.width() > 0.0 {
                if let Some(p) = seek_resp.interact_pointer_pos() {
                    let target = (((p.x - seek_rect.left()) / seek_rect.width()).clamp(0.0, 1.0) * duration).min(max_seek);
                    self.current_pos = target;
                    if duration - target > 4.0 { self.crossfade_triggered_for = None; }
                    if let Ok(mut pl) = self.audio_state.pos_sec.lock() { *pl = target; }
                    self.audio_state.finished_seq.store(0, Ordering::SeqCst);
                    let _ = self.audio_tx.send(AudioCmd::Seek(target));
                    self.sync_discord_presence();
                }
            }

            let is_hov = seek_resp.hovered() || dragging_now;
            let th = (if is_hov { 6.2 } else { 4.2 + beat * 1.4 }) * scale.sqrt();
            let bg_line = egui::Rect::from_center_size(seek_rect.center(), Vec2::new(bar_w, th));
            ui.painter().rect_filled(bg_line, Rounding::same(3.0), c_bg_widget());

            let cur_ratio = (self.current_pos / duration).clamp(0.0, 1.0);
            let cur_x = bg_line.left() + bar_w * cur_ratio;
            if is_hov {
                if let Some(hp) = ctx.input(|i| i.pointer.hover_pos()).or_else(|| seek_resp.interact_pointer_pos()) {
                    let hr = ((hp.x - bg_line.left()) / bar_w).clamp(0.0, 1.0);
                    let hx = bg_line.left() + bar_w * hr;
                    let hl_r = egui::Rect::from_min_max(egui::pos2(cur_x.min(hx), bg_line.top()), egui::pos2(cur_x.max(hx), bg_line.bottom()));
                    ui.painter().rect_filled(hl_r, Rounding::same(3.0), ThemePalette::ca(pal.accent_neon, 95));
                    let bdg = egui::Rect::from_center_size(egui::pos2(hx, bg_line.top() - 16.0), Vec2::new(46.0, 20.0));
                    ui.painter().rect(bdg, Rounding::same(5.0), c_bg_bar(), Stroke::new(1.0, c_neon()));
                    ui.painter().text(bdg.center(), egui::Align2::CENTER_CENTER, format_time(hr * duration), FontId::proportional(11.0), c_bright());
                }
            }
            if bar_w * cur_ratio > 0.5 {
                let fr = egui::Rect::from_min_size(bg_line.min, Vec2::new(bar_w * cur_ratio, th));
                ui.painter().rect_filled(fr, Rounding::same(3.0), c_neon());
            }
            if is_hov { ui.painter().circle_filled(egui::pos2(cur_x, bg_line.center().y), 5.5 * scale.sqrt(), c_bright()); }

            ui.add_space(gap_y * 0.95);
            self.render_media_controls_row(ui, ctx, btn_sm, btn_lg, liked);
        });
    }

    fn render_media_controls_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, btn_sm: f32, btn_lg: f32, liked: bool) {
        let spacing = (btn_sm * 0.26).clamp(8.0, 15.0);
        let (_, ctrl_rect) = ui.allocate_space(Vec2::new(btn_sm * 8.0 + btn_lg + spacing * 8.0, btn_lg + 4.0));
        let cy = ctrl_rect.center().y;
        let mut x = ctrl_rect.left();
        let is_wave = self.playback_ctx == PlaybackContext::Wave;
        let is_ym = self.current_track.as_ref().map(|t| t.provider == TrackProvider::Yandex && t.local_path.is_none()).unwrap_or(false);
        let is_local = self.current_track.as_ref().map(|t| t.local_path.is_some()).unwrap_or(true);
        let ic_map = self.skin_assets.icons.clone();
        let ic = |k: &str| ic_map.get(k);

        let r_lrc = draw_media_icon_btn(ui, "b_lrc", egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm)), MediaIcon::Lyrics(self.show_lyrics), false, !is_ym, ic("lyrics"));
        show_nl_tooltip(&r_lrc, if is_ym { "Текст песни (Караоке)" } else { "Караоке доступно для треков Яндекс Музыки" });
        if r_lrc.clicked() { self.show_lyrics = !self.show_lyrics; if self.show_lyrics { self.request_lyrics_for_current(ctx); } }
        x += btn_sm + spacing;

        let r_shuf = draw_media_icon_btn(ui, "b_shuf", egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm)), MediaIcon::Shuffle(self.config.shuffle && !is_wave), false, is_wave, ic("shuffle"));
        show_nl_tooltip(&r_shuf, "Перемешать (Shuffle)");
        if r_shuf.clicked() { self.toggle_shuffle(); }
        x += btn_sm + spacing;

        let r_like = draw_media_icon_btn(ui, "b_like", egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm)), MediaIcon::Heart(liked), false, !is_ym, ic("like"));
        show_nl_tooltip(&r_like, "Мне нравится [YM]");
        if r_like.clicked() { self.toggle_current_like(); }
        x += btn_sm + spacing;

        if draw_media_icon_btn(ui, "b_prev", egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm)), MediaIcon::Prev, false, false, ic("prev")).clicked() { self.step_prev_or_restart(ctx); }
        x += btn_sm + spacing;

        let (p_ic, p_key) = if self.is_playing { (MediaIcon::Pause, "pause") } else { (MediaIcon::Play, "play") };
        if draw_media_icon_btn(ui, "b_play", egui::Rect::from_center_size(egui::pos2(x + btn_lg * 0.5, cy), Vec2::splat(btn_lg)), p_ic, true, false, ic(p_key)).clicked() { self.toggle_play_pause(ctx); }
        x += btn_lg + spacing;

        if draw_media_icon_btn(ui, "b_next", egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm)), MediaIcon::Next, false, false, ic("next")).clicked() { self.step_next(false, false, ctx); }
        x += btn_sm + spacing;

        let rep_m = if is_wave { RepeatMode::Off } else { self.config.repeat };
        let r_rep = draw_media_icon_btn(ui, "b_rep", egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm)), MediaIcon::Repeat(rep_m), false, is_wave, ic("repeat"));
        show_nl_tooltip(&r_rep, "Режим повтора");
        if r_rep.clicked() { self.cycle_repeat(); }
        x += btn_sm + spacing;

        let r_dl = draw_media_icon_btn(ui, "b_dl", egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm)), MediaIcon::Download, false, is_local, ic("download"));
        show_nl_tooltip(&r_dl, "Сохранить аудиофайл в локальную папку (.mp3)");
        if r_dl.clicked() { self.export_current_mp3(ctx); }
        x += btn_sm + spacing;

        let vb_r = egui::Rect::from_center_size(egui::pos2(x + btn_sm * 0.5, cy), Vec2::splat(btn_sm));
        let pop_r = egui::Rect::from_center_size(egui::pos2(vb_r.center().x, vb_r.top() - 66.0), Vec2::new(38.0, 112.0));
        let ptr = ctx.input(|i| i.pointer.hover_pos());
        let drag_v = ui.input(|i| i.pointer.primary_down()) && ctx.data(|d| d.get_temp::<bool>(egui::Id::new("v_drag")).unwrap_or(false));

        if draw_media_icon_btn(ui, "b_vol", vb_r, MediaIcon::Volume(self.config.volume), false, false, ic("volume")).clicked() {
            self.config.volume = if self.config.volume > 0.01 { 0.0 } else { 0.7 };
            let _ = self.audio_tx.send(AudioCmd::SetVolume(self.config.volume));
            self.config.save();
        }
        if ptr.map(|p| vb_r.union(pop_r).expand(6.0).contains(p)).unwrap_or(false) || drag_v {
            egui::Area::new(egui::Id::new("vol_hover_popup")).order(egui::Order::Foreground).fixed_pos(pop_r.min).show(ctx, |ui| {
                let (pr, presp) = ui.allocate_exact_size(pop_r.size(), Sense::click_and_drag());
                ui.painter().rect(pr, Rounding::same(18.0), c_bg_bar(), Stroke::new(1.0, c_border_dark()));
                let (ty, by) = (pr.top() + 14.0, pr.bottom() - 14.0);
                let h = (by - ty).max(1.0);
                if presp.drag_started() { ctx.data_mut(|d| d.insert_temp(egui::Id::new("v_drag"), true)); }
                if presp.clicked() || presp.dragged() {
                    if let Some(pos) = presp.interact_pointer_pos() {
                        self.config.volume = 1.0 - ((pos.y - ty) / h).clamp(0.0, 1.0);
                        let _ = self.audio_tx.send(AudioCmd::SetVolume(self.config.volume));
                        self.config.save();
                    }
                }
                if !ui.input(|i| i.pointer.primary_down()) { ctx.data_mut(|d| d.insert_temp(egui::Id::new("v_drag"), false)); }
                let scr = ui.input(|i| i.smooth_scroll_delta.y);
                if scr.abs() > 0.1 {
                    self.config.volume = (self.config.volume + scr * 0.002).clamp(0.0, 1.0);
                    let _ = self.audio_tx.send(AudioCmd::SetVolume(self.config.volume));
                    self.config.save();
                }
                let cx = pr.center().x;
                ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(cx - 2.5, ty), egui::pos2(cx + 2.5, by)), Rounding::same(2.5), c_bg_widget());
                let fh = h * self.config.volume.clamp(0.0, 1.0);
                if fh > 0.5 { ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(cx - 2.5, by - fh), egui::pos2(cx + 2.5, by)), Rounding::same(2.5), c_bright()); }
            });
        }
    }
}
