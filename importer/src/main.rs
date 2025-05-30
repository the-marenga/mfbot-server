use std::{io::Cursor, sync::atomic::AtomicI32};

use chrono::{DateTime, NaiveDateTime, Utc};
use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sqlx::{prelude::FromRow, sqlite::*};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let client = Client::new();
    let options = SqliteConnectOptions::new()
        .filename(env!("DATABASE_URL").split_once(":").unwrap().1)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        // .auto_vacuum(sqlx::sqlite::SqliteAutoVacuum::Incremental)
        // .foreign_keys(true)
        // .optimize_on_close(true, Some(u32::MAX))
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        // .max_connections(50)
        // .min_connections(1)
        .connect_with(options)
        .await
        .unwrap();

    let ids = sqlx::query_scalar!("SELECT player_id FROM player_info",)
        .fetch_all(&pool)
        .await
        .unwrap();

    let tasks = ids.chunks(500).map(|chunk| {
        let client = client.clone();
        let pool = pool.clone();

        async move {
            let mut players = vec![];
            for player_id in chunk {
                let player = sqlx::query!(
                    "SELECT p.player_id, p.name as player_name, s.url as \
                     server, o.otherplayer_resp as info, description, \
                     i.guild_id, soldier_advice, fetch_time
                FROM player_info i
                JOIN player p ON p.player_id = i.player_id
                JOIN description d ON d.description_id = i.description_id
                JOIN server s on s.server_id = p.server_id
                JOIN otherplayer_resp o ON o.otherplayer_resp_id = \
                     i.otherplayer_resp_id
                WHERE i.player_id = ?
                ",
                    player_id
                )
                .fetch_one(&pool)
                .await
                .unwrap();
                players.push(player)
            }

            if players.is_empty() {
                return;
            }

            let mut new = vec![];
            for player in players {
                let data =
                    zstd::stream::decode_all(Cursor::new(player.info)).unwrap();
                let timestamp = player.fetch_time;
                let naive =
                    NaiveDateTime::from_timestamp_opt(timestamp, 0).unwrap();
                let datetime: DateTime<Utc> =
                    DateTime::<Utc>::from_utc(naive, Utc);

                let guild = match player.guild_id {
                    Some(guild_id) => sqlx::query_scalar!(
                        "SELECT name FROM guild WHERE guild_id = ?",
                        guild_id
                    )
                    .fetch_optional(&pool)
                    .await
                    .ok()
                    .flatten(),
                    None => None,
                };

                let player = RawOtherPlayer {
                    name: player.player_name,
                    server: player.server,
                    info: String::from_utf8(data).unwrap(),
                    description: player.description,
                    guild,
                    soldier_advice: Some(player.soldier_advice),
                    fetch_date: datetime.to_rfc3339(),
                    player_id: player.player_id,
                };
                new.push(player);
            }

            client
                .post("http://localhost:4949/report_players")
                .json(&new)
                .send()
                .await
                .unwrap();
        }
    });

    use futures::stream::StreamExt;
    let shared = AtomicI32::new(1);
    futures::stream::iter(tasks).buffer_unordered(50).for_each(|_| async {
        let v = shared.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        log::info!("{v}")
    }).await;
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RawOtherPlayer {
    pub name: String,
    pub server: String,
    pub info: String,
    pub description: Option<String>,
    pub guild: Option<String>,
    pub soldier_advice: Option<i64>,
    pub fetch_date: String,
    #[serde(skip)]
    pub player_id: i64,
}
