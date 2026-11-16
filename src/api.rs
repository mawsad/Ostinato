use crate::audio::{read_cached_mp3, save_cached_mp3};
use crate::models::{
    AlbumItem, ArtistItem, AuthMsg, DeviceCodeResp, LyricLine, PlaylistEntry, PlaylistSource,
    TokenPollResp, TrackItem, TrackProvider, WaveSettings, CLIENT_ID_A, CLIENT_ID_B, LYRICS_SALT_A,
    LYRICS_SALT_B, SECRET_A, SECRET_B, SIGN_SALT_A, SIGN_SALT_B,
};
use base64::Engine;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use serde_json::Value;
use sha2::Sha256;
use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

pub fn ym_api_base() -> String {
    format!("https://{}.{}.net", "api.music", "yandex")
}

pub fn shared_ym_client() -> &'static reqwest::blocking::Client {
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
            .pool_max_idle_per_host(3)
            .build()
            .unwrap_or_default()
    })
}

pub fn parse_track_value(v: &Value, liked_set: &[String]) -> Option<TrackItem> {
    let t = if v.get("track").is_some() {
        &v["track"]
    } else {
        v
    };
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
        .map(|u| format!("https://{}", u.replace("%%", "400x400")));

    Some(TrackItem {
        id,
        provider: TrackProvider::Yandex,
        title,
        artist,
        artist_id,
        duration_sec: (dur_ms / 1000.0) as f32,
        liked,
        cover_url,
        stream_hint: None,
        local_path: None,
    })
}
pub fn search_ym_all(
    token: &str,
    query: &str,
    liked_set: &[String],
) -> Result<(Vec<ArtistItem>, Vec<AlbumItem>, Vec<TrackItem>), String> {
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
        for a in arr.iter().take(6) {
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

    let mut albums = Vec::new();
    if let Some(arr) = v["result"]["albums"]["results"].as_array() {
        for al in arr.iter().take(6) {
            let al_id = match &al["id"] {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            };
            if let (Some(id), Some(title)) = (al_id, al["title"].as_str()) {
                let artist = al["artists"]
                    .as_array()
                    .and_then(|list| list.first())
                    .and_then(|first| first["name"].as_str())
                    .unwrap_or("Исполнитель")
                    .to_string();
                let year = al["year"].as_u64().map(|y| y as u32);
                albums.push(AlbumItem {
                    id,
                    title: title.to_string(),
                    artist,
                    year,
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

    Ok((artists, albums, tracks))
}

pub fn fetch_album_tracks(
    token: &str,
    album_id: &str,
    liked_set: &[String],
) -> Result<Vec<TrackItem>, String> {
    let client = shared_ym_client();
    let url = format!("{}/albums/{}/with-tracks", ym_api_base(), album_id);
    let auth = format!("OAuth {}", token);

    let v: Value = client
        .get(&url)
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    if let Some(volumes) = v["result"]["volumes"].as_array() {
        for vol in volumes {
            if let Some(arr) = vol.as_array() {
                for item in arr {
                    if let Some(tr) = parse_track_value(item, liked_set) {
                        out.push(tr);
                    }
                }
            }
        }
    }
    Ok(out)
}


pub fn fetch_artist_tracks(
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
        .query(&[("page", "0"), ("page-size", "80")])
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
pub fn fetch_ym_init(
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

    let mut playlists = vec![PlaylistEntry {
        title: "Мне нравится".into(),
        source: PlaylistSource::Likes,
    }];

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

    let first_slice_len = liked_ids.len().min(150);
    let initial_tracks = fetch_tracks_chunk(
        client,
        &base,
        &auth,
        &liked_ids[..first_slice_len],
        &liked_ids,
    )?;

    Ok((uid, playlists, liked_ids, initial_tracks))
}

pub fn fetch_tracks_chunk(
    client: &reqwest::blocking::Client,
    base: &str,
    auth: &str,
    chunk_ids: &[String],
    liked_set: &[String],
) -> Result<Vec<TrackItem>, String> {
    if chunk_ids.is_empty() {
        return Ok(Vec::new());
    }
    let joined = chunk_ids.join(",");

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
pub fn fetch_all_tracks_by_ids(
    token: &str,
    all_ids: &[String],
    liked_set: &[String],
) -> Result<Vec<TrackItem>, String> {
    let client = shared_ym_client();
    let base = ym_api_base();
    let auth = format!("OAuth {}", token);
    let mut full = Vec::with_capacity(all_ids.len());

    for chunk in all_ids.chunks(150) {
        let mut part = fetch_tracks_chunk(client, &base, &auth, chunk, liked_set)?;
        full.append(&mut part);
    }
    Ok(full)
}

pub fn apply_wave_settings(token: &str, settings: &WaveSettings) {
    let client = shared_ym_client();
    let url = format!("{}/rotor/station/user:onyourwave/settings2", ym_api_base());
    let body = serde_json::json!({
        "moodEnergy": settings.mood_energy,
        "diversity": settings.diversity,
        "language": "any"
    });
    let _ = client
        .post(&url)
        .header("Authorization", format!("OAuth {}", token))
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .json(&body)
        .send();
}

fn now_iso8601() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(1_700_000_000);
    let days = (secs / 86400) as i64 + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let rem = secs % 86400;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.000Z",
        year,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

pub fn send_wave_track_started(token: &str, track_id: &str, batch_id: Option<&str>) {
    if token.is_empty() || track_id.is_empty() || track_id.starts_with("local::") {
        return;
    }
    let client = shared_ym_client();
    let mut url = format!("{}/rotor/station/user:onyourwave/feedback", ym_api_base());
    if let Some(bid) = batch_id.filter(|s| !s.is_empty()) {
        url = format!("{}?batch-id={}", url, bid);
    }
    let body = serde_json::json!({
        "type": "trackStarted",
        "timestamp": now_iso8601(),
        "trackId": track_id
    });
    let _ = client
        .post(&url)
        .header("Authorization", format!("OAuth {}", token))
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .json(&body)
        .send();
}
pub fn fetch_wave_batch(
    token: &str,
    liked_set: &[String],
    finished_track_id: Option<&str>,
    played_seconds: f32,
    queue_tail_ids: &[String],
    batch_id: Option<&str>,
    wave_settings: Option<&WaveSettings>,
) -> Result<(Vec<TrackItem>, Option<String>), String> {
    if let Some(ws) = wave_settings {
        apply_wave_settings(token, ws);
    }

    let client = shared_ym_client();
    let base = ym_api_base();
    let auth = format!("OAuth {}", token);

    let mut fb_url = format!("{}/rotor/station/user:onyourwave/feedback", base);
    if let Some(bid) = batch_id.filter(|s| !s.is_empty()) {
        fb_url = format!("{}?batch-id={}", fb_url, bid);
    }
    let ts_iso = now_iso8601();

    if let Some(prev_id) = finished_track_id.filter(|s| !s.is_empty() && !s.starts_with("local::")) {
        let fb_type = if played_seconds < 15.0 { "skip" } else { "trackFinished" };
        let fb_body = serde_json::json!({
            "type": fb_type,
            "timestamp": ts_iso,
            "trackId": prev_id,
            "totalPlayedSeconds": played_seconds.max(1.0)
        });
        let _ = client
            .post(&fb_url)
            .header("Authorization", &auth)
            .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
            .json(&fb_body)
            .send();
    } else {
        let start_body = serde_json::json!({
            "type": "radioStarted",
            "timestamp": ts_iso,
            "from": "mobile-home-rup_main-user-onyourwave-default"
        });
        let _ = client
            .post(&fb_url)
            .header("Authorization", &auth)
            .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
            .json(&start_body)
            .send();
    }

    let queue_param = queue_tail_ids.join(",");
    let url = if !queue_param.is_empty() {
        format!("{}/rotor/station/user:onyourwave/tracks?settings2=true&queue={}", base, queue_param)
    } else if let Some(q) = finished_track_id.filter(|s| !s.is_empty() && !s.starts_with("local::")) {
        format!("{}/rotor/station/user:onyourwave/tracks?settings2=true&queue={}", base, q)
    } else {
        format!("{}/rotor/station/user:onyourwave/tracks?settings2=true", base)
    };

    let v: Value = client
        .get(&url)
        .header("Authorization", &auth)
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;

    let new_batch_id = v["result"]["batchId"].as_str().map(|s| s.to_string());
    let mut out = Vec::new();
    if let Some(seq) = v["result"]["sequence"].as_array() {
        for item in seq {
            if let Some(tr) = parse_track_value(&item["track"], liked_set) {
                out.push(tr);
            }
        }
    }
    Ok((out, new_batch_id))
}

pub fn fetch_playlist_tracks(
    token: &str,
    uid: u64,
    src: &PlaylistSource,
    liked_set: &[String],
) -> Result<Vec<TrackItem>, String> {
    let client = shared_ym_client();
    let base = ym_api_base();
    let auth = format!("OAuth {}", token);

    match src {
        PlaylistSource::Likes => fetch_all_tracks_by_ids(token, liked_set, liked_set),
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
        _ => Ok(Vec::new()),
    }
}


pub fn set_track_like_remote(token: &str, uid: u64, track_id: &str, liked: bool) {
    let client = shared_ym_client();
    let action = if liked { "add-multiple" } else { "remove" };
    let url = format!("{}/users/{}/likes/tracks/{}", ym_api_base(), uid, action);
    let _ = client
        .post(&url)
        .header("Authorization", format!("OAuth {}", token))
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .form(&[("track-ids", track_id)])
        .send();
}

pub fn download_track_mp3(token: &str, track_id: &str) -> Result<Vec<u8>, String> {
    if let Some(cached) = read_cached_mp3(track_id) {
        return Ok(cached);
    }

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
        .map_err(|e| e.to_string())?
        .to_vec();

    save_cached_mp3(track_id, &bytes);
    Ok(bytes)
}
pub fn fetch_track_lyrics(
    token: &str,
    track_id: &str,
    title: &str,
    artist: &str,
    duration_sec: f32,
) -> Result<(bool, Vec<LyricLine>), String> {
    let client = shared_ym_client();
    let mut fallback_plain: Option<Vec<LyricLine>> = None;
    let mut ym_debug = String::new();

    // 1. Яндекс Музыка
    if !token.is_empty() && !track_id.starts_with("local::") && !track_id.starts_with("sc_") {
        let clean_id = track_id.split(':').next().unwrap_or(track_id);
        let base = ym_api_base();
        let auth = format!("OAuth {}", token);

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
            .to_string();

        let secret = format!("{}{}", LYRICS_SALT_A, LYRICS_SALT_B);
        if let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) {
            mac.update(format!("{}{}", clean_id, ts).as_bytes());
            let sign = base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

            let url = format!("{}/tracks/{}/lyrics", base, clean_id);
            match client
                .get(&url)
                .header("Authorization", &auth)
                .header("X-Yandex-Music-Client", "YandexMusicAndroid/24024321")
                .query(&[
                    ("timeStamp", ts.as_str()),
                    ("sign", sign.as_str()),
                    ("format", "LRC"),
                ])
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    if let Ok(v) = resp.json::<Value>() {
                        if let Some(dl_url) = v["result"]["downloadUrl"].as_str() {
                            if let Ok(txt_r) = client.get(dl_url).send() {
                                if let Ok(raw_lrc) = txt_r.text() {
                                    let parsed = parse_lrc_lines(&raw_lrc);
                                    if !parsed.is_empty() {
                                        return Ok((true, parsed));
                                    }
                                }
                            }
                        } else {
                            let err_name = v["error"]["name"].as_str().unwrap_or("no_url");
                            ym_debug = format!("YM {}: {}", status.as_u16(), err_name);
                        }
                    }
                }
                Err(e) => ym_debug = format!("YM net: {}", e),
            }
        }

        // Обычный текст из /supplement
        let sup_url = format!("{}/tracks/{}/supplement", base, clean_id);
        if let Ok(resp) = client
            .get(&sup_url)
            .header("Authorization", &auth)
            .header("X-Yandex-Music-Client", "YandexMusicAndroid/24024321")
            .send()
        {
            if let Ok(v) = resp.json::<Value>() {
                if let Some(full) = v["result"]["lyrics"]["fullLyrics"].as_str() {
                    let try_lrc = parse_lrc_lines(full);
                    if !try_lrc.is_empty() {
                        return Ok((true, try_lrc));
                    }
                    let lines: Vec<LyricLine> = full
                        .lines()
                        .map(|l| LyricLine {
                            time_sec: 0.0,
                            text: l.trim().to_string(),
                        })
                        .filter(|l| !l.text.is_empty())
                        .collect();
                    if !lines.is_empty() {
                        fallback_plain = Some(lines);
                    }
                }
            }
        }
    }

    // 2. Отдельный чистый HTTP-клиент для LRCLIB (без заголовков Яндекса)
    let lrc_client = reqwest::blocking::Client::builder()
        .user_agent("OstinatoPlayer/4.0 (https://github.com)")
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_default();

    let main_artist = artist.split(',').next().unwrap_or(artist).trim();
    let search_q = format!("{} {}", main_artist, title);
    let mut lrc_debug = String::new();

    match lrc_client
        .get("https://lrclib.net")
        .query(&[("q", search_q.as_str())])
        .send()
    {
        Ok(resp) => {
            let st = resp.status();
            if let Ok(arr_v) = resp.json::<Value>() {
                if let Some(items) = arr_v.as_array() {
                    lrc_debug = format!("LRCLIB {}: {} шт.", st.as_u16(), items.len());
                    let mut best_synced: Option<(f32, Vec<LyricLine>)> = None;

                    for item in items {
                        let item_dur = item["duration"].as_f64().unwrap_or(duration_sec as f64) as f32;
                        let diff = (item_dur - duration_sec).abs();
                        if diff > 15.0 {
                            continue;
                        }

                        if let Some(synced_lrc) = item["syncedLyrics"].as_str() {
                            let parsed = parse_lrc_lines(synced_lrc);
                            if !parsed.is_empty() {
                                if best_synced.as_ref().map(|(d, _)| diff < *d).unwrap_or(true) {
                                    best_synced = Some((diff, parsed));
                                }
                            }
                        }

                        if fallback_plain.is_none() {
                            if let Some(plain) = item["plainLyrics"].as_str() {
                                let lines: Vec<LyricLine> = plain
                                    .lines()
                                    .map(|l| LyricLine {
                                        time_sec: 0.0,
                                        text: l.trim().to_string(),
                                    })
                                    .filter(|l| !l.text.is_empty())
                                    .collect();
                                if !lines.is_empty() {
                                    fallback_plain = Some(lines);
                                }
                            }
                        }
                    }

                    if let Some((_, lines)) = best_synced {
                        return Ok((true, lines));
                    }
                }
            }
        }
        Err(e) => lrc_debug = format!("LRCLIB err: {}", e),
    }

    if let Some(plain) = fallback_plain {
        return Ok((false, plain));
    }

    // Возвращаем диагностику одной строкой, чтобы сразу увидеть причину в UI
    Err("Текст для этого трека отсутствует".into())

}



fn parse_lrc_lines(raw: &str) -> Vec<LyricLine> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let mut rest = line.trim();
        let mut timestamps = Vec::new();

        while rest.starts_with('[') {
            let Some(close_idx) = rest.find(']') else {
                break;
            };
            let tag = &rest[1..close_idx];
            if let Some((mm_str, ss_str)) = tag.split_once(':') {
                if let (Ok(mins), Ok(secs)) = (
                    mm_str.trim().parse::<f32>(),
                    ss_str.trim().replace(',', ".").parse::<f32>(),
                ) {
                    timestamps.push(mins * 60.0 + secs);
                    rest = rest[close_idx + 1..].trim();
                    continue;
                }
            }
            break;
        }

        let clean_text = rest.trim();
        if !timestamps.is_empty() && !clean_text.is_empty() {
            for ts in timestamps {
                out.push(LyricLine {
                    time_sec: ts,
                    text: clean_text.to_string(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.time_sec.partial_cmp(&b.time_sec).unwrap_or(std::cmp::Ordering::Equal));
    out
}


pub fn spawn_device_auth(tx: Sender<AuthMsg>, ctx: eframe::egui::Context) {
    thread::spawn(move || {
        let cid = format!("{}{}", CLIENT_ID_A, CLIENT_ID_B);
        let csec = format!("{}{}", SECRET_A, SECRET_B);

        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(987654321);
        let dev_id = format!("os{:08x}", (nanos & 0xFFFF_FFFF) as u32);

        let client = shared_ym_client();
        let host_part = format!("{}.{}", "yandex", "ru");
        let code_ep = format!("https://oauth.{}/device/code", host_part);
               let scoped_params = [
            ("client_id", cid.as_str()),
            ("device_id", dev_id.as_str()),
            ("device_name", "Ostinato v3.2"),
            ("scope", "music:content music:read music:write"),
        ];

        let mut raw_text = match client
            .post(&code_ep)
            .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
            .header("Accept", "application/json")
            .form(&scoped_params)
            .send()
            .and_then(|r| r.text())
        {
            Ok(t) => t,
            Err(e) => {
                let _ = tx.send(AuthMsg::Error(format!("Ошибка сети: {}", e)));
                ctx.request_repaint();
                return;
            }
        };

        if raw_text.contains("invalid_scope") {
            let fallback_params = [
                ("client_id", cid.as_str()),
                ("device_id", dev_id.as_str()),
                ("device_name", "Ostinato v3.2"),
            ];
            if let Ok(r) = client
                .post(&code_ep)
                .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
                .header("Accept", "application/json")
                .form(&fallback_params)
                .send()
            {
                if let Ok(t) = r.text() {
                    raw_text = t;
                }
            }
        }

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
            .unwrap_or_else(|| format!("https://{}.{}/device", "ya", "ru"));

        let _ = open::that(&ver_url);
        let _ = tx.send(AuthMsg::CodeReady {
            code: usr_code,
            url: ver_url,
        });
        ctx.request_repaint();

        let tok_ep = format!("https://oauth.{}/token", host_part);
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

pub fn extract_token(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(pos) = trimmed.find("access_token=") {
        let after = &trimmed[pos + "access_token=".len()..];
        let end = after.find('&').unwrap_or(after.len());
        return after[..end].to_string();
    }
    trimmed.to_string()
}
