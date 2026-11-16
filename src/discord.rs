use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const DISCORD_APP_ID: &str = "1571703761502609458";

pub enum DiscordCmd {
    Playing {
        title: String,
        artist: String,
        provider: &'static str,
        duration_sec: f32,
        pos_sec: f32,
        cover_url: Option<String>,
    },
    Paused {
        title: String,
        artist: String,
    },
    Clear,
}

pub fn spawn_discord_rpc() -> Sender<DiscordCmd> {
    let (tx, rx): (Sender<DiscordCmd>, Receiver<DiscordCmd>) = mpsc::channel();

    thread::spawn(move || {
        let Ok(mut client) = DiscordIpcClient::new(DISCORD_APP_ID) else {
            return;
        };
        let mut connected = false;

        while let Ok(cmd) = rx.recv() {
            let mut latest_cmd = cmd;
            while let Ok(newer) = rx.try_recv() {
                latest_cmd = newer;
            }

            if !connected {
                if client.connect().is_ok() {
                    connected = true;
                } else {
                    continue;
                }
            }

            let res = match latest_cmd {
                DiscordCmd::Playing {
                    title,
                    artist,
                    provider,
                    duration_sec,
                    pos_sec,
                    cover_url,
                } => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    let start_ts = now - (pos_sec.max(0.0) as i64);
                    let end_ts = start_ts + (duration_sec.max(1.0) as i64);

                    let state_str = format!("{}  [{}]", artist, provider);
                    let mut assets = activity::Assets::new().large_text("Ostinato v4.0");
                    if let Some(url) = cover_url.as_deref().filter(|u| u.starts_with("http")) {
                        assets = assets.large_image(url);
                    }

                    let act = activity::Activity::new()
                        .activity_type(activity::ActivityType::Listening)
                        .details(&title)
                        .state(&state_str)
                        .timestamps(activity::Timestamps::new().start(start_ts).end(end_ts))
                        .assets(assets);

                    client.set_activity(act)
                }
                DiscordCmd::Paused { title, artist } => {
                    let state_str = format!("На паузе — {}", artist);
                    let act = activity::Activity::new()
                        .activity_type(activity::ActivityType::Listening)
                        .details(&title)
                        .state(&state_str);
                    client.set_activity(act)
                }
                DiscordCmd::Clear => client.clear_activity(),
            };

            if res.is_err() {
                let _ = client.close();
                connected = false;
            }
        }
    });

    tx
}
