use crate::api::download_track_mp3;
use crate::audio::{read_cached_mp3, save_cached_mp3};
use crate::models::{AuthMsg, TrackItem, TrackProvider};
use serde_json::Value;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

fn sc_http_client() -> &'static reqwest::blocking::Client {
    static CL: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CL.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .connect_timeout(Duration::from_secs(6))
            .timeout(Duration::from_secs(14))
            .build()
            .unwrap_or_default()
    })
}

fn vk_client() -> &'static reqwest::blocking::Client {
    static VK_CL: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    VK_CL.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent("VKAndroidApp/5.52-4543 (Android 9; SDK 28; arm64-v8a; Xiaomi Mi 9; ru; 1080x2340)")
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default()
    })
}


fn sc_block_hint() -> String {
    let domain = ["sound", "cloud.com"].concat();
    format!("SoundCloud: добавьте {} в Zapret или включите VPN", domain)
}


pub fn spawn_vk_device_auth(tx: Sender<AuthMsg>, ctx: eframe::egui::Context) {
    thread::spawn(move || {
        let cl = vk_client();
        let resp_txt = match cl
            .get("https://vk.com")
            .query(&[
                ("scope", "audio,offline"),
                ("client_id", "2685278"),
                ("device_name", "Ostinato Player"),
            ])
            .send()
            .and_then(|r| r.text())
        {
            Ok(t) => t,
            Err(e) => {
                let _ = tx.send(AuthMsg::Error(format!("VK сеть: {}", e)));
                ctx.request_repaint();
                return;
            }
        };

        let Ok(v) = serde_json::from_str::<Value>(&resp_txt) else {
            let _ = tx.send(AuthMsg::Error("Ошибка ответа сервера кодов VK".into()));
            ctx.request_repaint();
            return;
        };

        let Some(auth_code) = v["auth_code"].as_str().map(|s| s.to_string()) else {
            let err = v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("VK отклонил запрос кода");
            let _ = tx.send(AuthMsg::Error(format!("VK: {}", err)));
            ctx.request_repaint();
            return;
        };
        let dev_id = v["device_id"].as_str().unwrap_or("").to_string();
        let ver_url = v["verification_uri"].as_str().unwrap_or("https://vk.com").to_string();
        let wait_sec = v["interval"].as_u64().unwrap_or(5).max(3);

        let _ = open::that(&ver_url);
        let _ = tx.send(AuthMsg::CodeReady { code: auth_code, url: ver_url });
        ctx.request_repaint();

        for _ in 1..=60 {
            thread::sleep(Duration::from_secs(wait_sec));
            if let Ok(r) = cl
                .get("https://vk.com")
                .query(&[("client_id", "2685278"), ("device_id", dev_id.as_str())])
                .send()
            {
                if let Ok(tv) = r.json::<Value>() {
                    if let Some(tok) = tv["access_token"].as_str() {
                        let _ = tx.send(AuthMsg::Success(tok.to_string()));
                        ctx.request_repaint();
                        return;
                    }
                    if let Some(err) = tv["error"].as_str() {
                        if err != "authorization_pending" && err != "slow_down" {
                            let _ = tx.send(AuthMsg::Error(format!("VK отказ: {}", err)));
                            ctx.request_repaint();
                            return;
                        }
                    }
                }
            }
        }
        let _ = tx.send(AuthMsg::Error("Время ожидания кода VK истекло".into()));
        ctx.request_repaint();
    });
}
static SC_CLIENT_ID: OnceLock<Mutex<Option<String>>> = OnceLock::new();

pub fn get_sc_client_id() -> Result<String, String> {
    let lock = SC_CLIENT_ID.get_or_init(|| Mutex::new(None));
    if let Ok(g) = lock.lock() {
        if let Some(cid) = g.clone() { return Ok(cid); }
    }
    let cl = sc_http_client();
    let sc_main = format!("{}://{}{}", "https", "sound", "cloud.com");
    let cdn_prefix = format!("src=\"{}://{}{}", "https", "a-v2.snd", "cdn.com");
    let html = cl.get(&sc_main).send().and_then(|r| r.text()).map_err(|_| sc_block_hint())?;

    let mut script_urls = Vec::new();
    for chunk in html.split(&cdn_prefix) {
        if let Some(end) = chunk.find(".js\"") {
            let tail = &chunk[..end];
            if tail.len() < 95 && !tail.contains('<') {
                let raw_prefix = cdn_prefix.trim_start_matches("src=\"");
                script_urls.push(format!("{}{}.js", raw_prefix, tail));
            }
        }
    }
    for url in script_urls.into_iter().rev().take(7) {
        if let Ok(js) = cl.get(&url).send().and_then(|r| r.text()) {
            for marker in ["client_id:\"", "client_id=\""] {
                if let Some(idx) = js.find(marker) {
                    let rest = &js[idx + marker.len()..];
                    if let Some(q_end) = rest.find('"') {
                        let cand = &rest[..q_end];
                        if (28..=40).contains(&cand.len()) && cand.chars().all(|c| c.is_ascii_alphanumeric()) {
                            if let Ok(mut g) = lock.lock() { *g = Some(cand.to_string()); }
                            return Ok(cand.to_string());
                        }
                    }
                }
            }
        }
    }
    Err(sc_block_hint())
}


fn parse_sc_track_obj(item: &Value) -> Option<TrackItem> {
    let id_num = item["id"].as_u64()?;
    let title = item["title"].as_str().unwrap_or("SoundCloud Track").to_string();
    let artist = item["user"]["username"].as_str().unwrap_or("SC Artist").to_string();
    let dur_ms = item["duration"].as_f64().unwrap_or(180_000.0).max(1000.0);
    let cover_url = item["artwork_url"].as_str().map(|u| u.replace("-large", "-t500x500"));

    let mut best_transcoding: Option<String> = None;
    if let Some(tr_arr) = item["media"]["transcodings"].as_array() {
        for tr in tr_arr {
            let proto = tr["format"]["protocol"].as_str().unwrap_or("");
            let mime = tr["format"]["mime_type"].as_str().unwrap_or("");
            if let Some(u) = tr["url"].as_str() {
                if proto == "progressive" && mime.contains("mpeg") {
                    best_transcoding = Some(u.to_string()); break;
                } else if best_transcoding.is_none() && mime.contains("mpeg") {
                    best_transcoding = Some(u.to_string());
                }
            }
        }
    }
    best_transcoding.map(|hint| TrackItem {
        id: format!("sc_{}", id_num), provider: TrackProvider::SoundCloud,
        title, artist, artist_id: None, duration_sec: (dur_ms / 1000.0) as f32,
        liked: false, cover_url, stream_hint: Some(hint), local_path: None,
    })
}

pub fn search_soundcloud(query: &str) -> Result<Vec<TrackItem>, String> {
    let cl = sc_http_client();
    let web_search = format!("{}://{}{}/search/sounds", "https", "sound", "cloud.com");
    if let Ok(resp) = cl.get(&web_search).query(&[("q", query)]).send() {
        if let Ok(html) = resp.text() {
            if let Some(pos) = html.find("window.__sc_hydration = ") {
                let after = &html[pos + "window.__sc_hydration = ".len()..];
                if let Some(end) = after.find(";</script>") {
                    if let Ok(hyd) = serde_json::from_str::<Value>(&after[..end]) {
                        let mut out = Vec::new();
                        if let Some(arr) = hyd.as_array() {
                            for entry in arr {
                                if entry["hydratable"].as_str() == Some("search") {
                                    if let Some(col) = entry["data"]["collection"].as_array() {
                                        for item in col {
                                            if let Some(tr) = parse_sc_track_obj(item) { out.push(tr); }
                                        }
                                    }
                                }
                            }
                        }
                        if !out.is_empty() { return Ok(out); }
                    }
                }
            }
        }
    }

    let cid = get_sc_client_id()?;
    let api_search = format!("{}://{}{}/search/tracks", "https", "api-v2.sound", "cloud.com");
    let v: Value = cl
        .get(&api_search)
        .query(&[("q", query), ("client_id", cid.as_str()), ("limit", "35")])
        .send().map_err(|_| sc_block_hint())?
        .json().map_err(|_| sc_block_hint())?;

    let mut out = Vec::new();
    if let Some(arr) = v["collection"].as_array() {
        for item in arr {
            if let Some(tr) = parse_sc_track_obj(item) { out.push(tr); }
        }
    }
    Ok(out)
}

pub fn download_hls_or_direct(client: &reqwest::blocking::Client, media_url: &str) -> Result<Vec<u8>, String> {
    if !media_url.contains(".m3u8") {
        let b = client.get(media_url).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes()).map_err(|e| e.to_string())?;
        return Ok(b.to_vec());
    }
    let m3u_text = client.get(media_url).send().and_then(|r| r.text()).map_err(|e| format!("HLS плейлист: {}", e))?;
    let base_prefix = media_url.rsplit_once('/').map(|(b, _)| b).unwrap_or("");
    let mut combined = Vec::with_capacity(3 * 1024 * 1024);

    for line in m3u_text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') { continue; }
        let seg_url = if l.starts_with("http://") || l.starts_with("https://") { l.to_string() } else { format!("{}/{}", base_prefix, l.trim_start_matches('/')) };
        if let Ok(resp) = client.get(&seg_url).send() {
            if let Ok(seg_bytes) = resp.bytes() { combined.extend_from_slice(&seg_bytes); }
        }
    }
    if combined.len() < 8192 { return Err("Поток HLS пуст или защищён шифрованием".into()); }
    Ok(combined)
}

fn parse_vk_items(v: &Value) -> Vec<TrackItem> {
    let mut out = Vec::new();
    if let Some(arr) = v["response"]["items"].as_array() {
        for item in arr {
            let url_str = item["url"].as_str().unwrap_or("").trim();
            if url_str.is_empty() { continue; }
            let owner_id = item["owner_id"].as_i64().unwrap_or(0);
            let aid = item["id"].as_i64().unwrap_or(0);
            let title = item["title"].as_str().unwrap_or("VK Track").to_string();
            let artist = item["artist"].as_str().unwrap_or("VK Artist").to_string();
            let dur = item["duration"].as_f64().unwrap_or(180.0).max(1.0) as f32;
            let cover = item["album"]["thumb"]["photo_300"].as_str().or_else(|| item["album"]["thumb"]["photo_270"].as_str()).map(|s| s.to_string());
            out.push(TrackItem {
                id: format!("vk_{}_{}", owner_id, aid), provider: TrackProvider::Vk,
                title, artist, artist_id: None, duration_sec: dur, liked: false,
                cover_url: cover, stream_hint: Some(url_str.to_string()), local_path: None,
            });
        }
    }
    out
}

pub fn fetch_vk_my_audio(vk_token: &str) -> Result<Vec<TrackItem>, String> {
    if vk_token.trim().is_empty() { return Err("Токен VK не указан".into()); }
    let ep = format!("{}://{}.{}/method/audio.get", "https", "api", "vk.com");
    let v: Value = vk_client()
        .get(&ep)
        .query(&[("access_token", vk_token.trim()), ("count", "300"), ("v", "5.131")])
        .send().map_err(|e| e.to_string())?.json().map_err(|e| e.to_string())?;
    if let Some(err_msg) = v["error"]["error_msg"].as_str() { return Err(format!("VK API: {}", err_msg)); }
    Ok(parse_vk_items(&v))
}

pub fn search_vk_audio(vk_token: &str, query: &str) -> Result<Vec<TrackItem>, String> {
    if vk_token.trim().is_empty() { return Err("Токен VK не указан во вкладке Аккаунты".into()); }
    let ep = format!("{}://{}.{}/method/audio.search", "https", "api", "vk.com");
    let v: Value = vk_client()
        .get(&ep)
        .query(&[("access_token", vk_token.trim()), ("q", query), ("count", "60"), ("auto_complete", "1"), ("v", "5.131")])
        .send().map_err(|e| e.to_string())?.json().map_err(|e| e.to_string())?;
    if let Some(err_msg) = v["error"]["error_msg"].as_str() { return Err(format!("VK API: {}", err_msg)); }
    Ok(parse_vk_items(&v))
}

pub fn download_any_track_bytes(track: &TrackItem, ym_token: &str, _vk_token: &str) -> Result<Vec<u8>, String> {
    if let Some(cached) = read_cached_mp3(&track.id) { return Ok(cached); }
    let bytes = match track.provider {
        TrackProvider::Yandex => download_track_mp3(ym_token, &track.id)?,
        TrackProvider::SoundCloud => {
            let cid = get_sc_client_id()?;
            let trans_url = track.stream_hint.as_deref().ok_or("Нет ссылки потока SC")?;
            let sep = if trans_url.contains('?') { "&" } else { "?" };
            let resolve_url = format!("{}{}client_id={}", trans_url, sep, cid);
            let v: Value = sc_http_client().get(&resolve_url).send().map_err(|_| sc_block_hint())?.json().map_err(|_| sc_block_hint())?;
            let final_url = v["url"].as_str().ok_or_else(sc_block_hint)?;
            let b = download_hls_or_direct(sc_http_client(), final_url).map_err(|_| sc_block_hint())?;
            save_cached_mp3(&track.id, &b);
            b
        }
        TrackProvider::Vk => {
            let raw_url = track.stream_hint.as_deref().ok_or("Нет ссылки потока VK")?;
            let b = download_hls_or_direct(vk_client(), raw_url)?;
            save_cached_mp3(&track.id, &b);
            b
        }
        TrackProvider::Local => return Err("Локальный файл читается напрямую".into()),
    };
    Ok(bytes)
}
#[cfg(windows)]
fn read_clipboard_win32() -> Option<String> {
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hWndNewOwner: *mut std::ffi::c_void) -> i32;
        fn GetClipboardData(uFormat: u32) -> *mut std::ffi::c_void;
        fn CloseClipboard() -> i32;
    }

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let h = GetClipboardData(13); // CF_UNICODETEXT
        let mut res = None;
        if !h.is_null() {
            let ptr = GlobalLock(h) as *const u16;
            if !ptr.is_null() {
                let mut len = 0;
                while *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                res = Some(String::from_utf16_lossy(slice));
                let _ = GlobalUnlock(h);
            }
        }
        CloseClipboard();
        res
    }
}


pub fn spawn_vk_direct_auth(login: String, password: String, code_2fa: String, tx: Sender<AuthMsg>, ctx: eframe::egui::Context) {
    thread::spawn(move || {
        let ep = format!("{}://{}.{}/token", "https", "oauth", "vk.com");
        let mut params = vec![
            ("grant_type", "password".to_string()),
            ("client_id", "6146827".to_string()),
            ("client_secret", "hHbZxrka2uZ6jB1inYsH".to_string()),
            ("username", login.trim().to_string()),
            ("password", password),
            ("v", "5.131".to_string()),
            ("2fa_supported", "1".to_string()),
        ];
        if !code_2fa.trim().is_empty() {
            params.push(("code", code_2fa.trim().to_string()));
        }

        let resp = vk_client().post(&ep).form(&params).send();
        match resp {
            Ok(r) => match r.json::<Value>() {
                Ok(v) => {
                    if let Some(tok) = v["access_token"].as_str() {
                        let _ = tx.send(AuthMsg::Success(tok.to_string()));
                    } else if let Some(err) = v["error"].as_str() {
                        if err == "need_validation" {
                            let _ = tx.send(AuthMsg::Error("Требуется код 2FA (введите код из SMS / приложения VK и нажмите Войти)".into()));
                        } else {
                            let desc = v["error_description"].as_str().unwrap_or(err);
                            let _ = tx.send(AuthMsg::Error(format!("VK: {}", desc)));
                        }
                    } else {
                        let _ = tx.send(AuthMsg::Error("Не удалось получить токен VK".into()));
                    }
                }
                Err(e) => { let _ = tx.send(AuthMsg::Error(e.to_string())); }
            },
            Err(e) => { let _ = tx.send(AuthMsg::Error(e.to_string())); }
        }
        ctx.request_repaint();
    });
}

