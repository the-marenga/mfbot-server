use std::{fmt::Write, time::Duration};

use axum::{
    Json, Router,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use chrono::Utc;
use db::{get_db, get_server_id};
use log::error;
use serde::{Deserialize, Serialize};
use sf_api::gamestate::{
    ServerTime,
    social::OtherPlayer,
    unlockables::{EquipmentIdent, ScrapBook},
};
use sqlx::prelude::FromRow;

pub mod db;

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(root))
        .route("/updatePlayers", post(report_players))
        .route("/scrapbookAdvice", post(scrapbook_advice))
        .route("/report", post(report_bug));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:4949").await?;
    Ok(axum::serve(listener, app).await?)
}

async fn root() -> axum::response::Redirect {
    axum::response::Redirect::permanent("https://forum.mfbot.de/")
}

/// Compresses the original Equipment Ident into a single i32
pub fn compress_ident(ident: EquipmentIdent) -> i32 {
    let mut res = ident.model_id as i64; // 0..16
    res |= (ident.color as i64) << 16; // 16..24
    res |= (ident.typ as i64) << 24; // 24..28
    res |= (ident.class.map(|a| a as i64 + 1).unwrap_or(0)) << 28; // 28..32
    res as i32
}

#[derive(Debug, Deserialize)]
pub struct ScrapBookAdviceArgs {
    raw_scrapbook: String,
    server: String,
    max_level: u16,
    max_attrs: u64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ScrapBookAdvice {
    player_name: String,
    new_count: u32,
}

pub async fn scrapbook_advice(
    Json(args): Json<ScrapBookAdviceArgs>,
) -> Result<Json<Vec<ScrapBookAdvice>>, Response> {
    let sb = ScrapBook::parse(&args.raw_scrapbook)
        .ok_or(MFBotError::InvalidScrapbook)?;
    let collected: Vec<_> = sb.items.into_iter().map(compress_ident).collect();

    let db = get_db().await?;
    let server_id = get_server_id(&db, args.server).await?;

    let mut filter = format!("WHERE server_id = {server_id} ");

    if !collected.is_empty() {
        filter.push_str("AND ident NOT IN (");
        for (pos, ident) in collected.into_iter().enumerate() {
            if pos > 0 {
                filter.push(',');
            }
            filter
                .write_fmt(format_args!("{ident}"))
                .map_err(|_| MFBotError::Internal)?
        }
        filter.push(')');
    }

    let sql = format!(
        "SELECT name, new_count
        FROM player
        NATURAL JOIN (
            SELECT player_id, count(*) as new_count
            FROM equipment
            {filter}
            GROUP BY player_id
        )
        WHERE level <= {} AND attributes <= {} AND is_removed = false
        ORDER BY new_count DESC, level ASC, attributes ASC
        LIMIT 25",
        args.max_level, args.max_attrs
    );

    // NOTE: The is basically doing this:
    //
    // SELECT name, COUNT(*)
    // FROM player
    // NATURAL JOIN equipment
    // WHERE server_id = ?
    //  AND level <= ?
    //  AND attributes <= ?
    //  AND is_removed = false
    //  AND ident NOT IN (...)
    // GROUP BY player_id
    // ORDER BY COUNT(*) DESC
    // LIMIT 25;
    //
    // The more readable query is a lot slower though, so we group equipment
    // first and then filter players

    Ok(Json(
        sqlx::query_as(&sql)
            .fetch_all(&db)
            .await
            .map_err(MFBotError::DBError)?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct RawOtherPlayer {
    name: String,
    server: String,
    info: String,
    description: Option<String>,
    guild: Option<String>,
    soldier_advice: Option<i64>,
    fetch_date: String,
}

async fn report_players(
    Json(players): Json<Vec<RawOtherPlayer>>,
) -> Result<(), Response> {
    let db = get_db().await?;
    for player in players {
        if let Err(err) = insert_player(&db, player).await {
            error!("{err}");
        }
    }
    Ok(())
}

async fn insert_player(
    db: &sqlx::Pool<sqlx::Sqlite>,
    player: RawOtherPlayer,
) -> Result<(), MFBotError> {
    log::info!("Player reported: {}@{}", player.name, player.server);
    let server_id = get_server_id(db, player.server).await?;
    let data: Result<Vec<i64>, _> =
        player.info.trim().split("/").map(|a| a.parse()).collect();
    let Ok(data) = data else {
        return Err(MFBotError::InvalidPlayer(
            format!("Could not parse player {}", player.name).into(),
        ));
    };
    let Ok(other) = OtherPlayer::parse(&data, ServerTime::default()) else {
        return Err(MFBotError::InvalidPlayer(
            format!("Could not parse player {}", player.name).into(),
        ));
    };
    let Ok(mut fetch_time) =
        chrono::DateTime::parse_from_rfc3339(&player.fetch_date)
            .map(|a| a.to_utc())
    else {
        return Err(MFBotError::InvalidPlayer(
            format!("Could not parse fetch date: {}", player.fetch_date).into(),
        ));
    };
    let now = Utc::now();
    if fetch_time > now {
        fetch_time = now;
    }

    let experience = other.experience as i64;

    let equip_idents: Vec<_> = other
        .equipment
        .0
        .values()
        .filter_map(|item| item.as_ref()?.equipment_ident().map(compress_ident))
        .collect();

    let attributes = other
        .base_attributes
        .values()
        .chain(other.bonus_attributes.values())
        .copied()
        .map(i64::from)
        .sum::<i64>();

    let equip_count = equip_idents.len() as i32;
    let mut tx = db.begin().await?;

    let existing = sqlx::query!(
        "SELECT player_id, level, attributes, last_reported, xp, last_changed
         FROM player
         WHERE server_id = ? AND name = ?",
        server_id,
        player.name
    )
    .fetch_optional(&mut *tx)
    .await?;

    let fetch_timestamp = fetch_time.timestamp();

    let pid = if let Some(existing) = existing {
        if existing.last_reported.is_some_and(|a| a >= fetch_timestamp) {
            log::warn!("Discarded player update for {}", player.name);
            return Ok(());
        }
        let has_changed = existing.attributes.is_none_or(|a| a != attributes)
            || existing.xp.is_none_or(|a| a != experience)
            || existing.level.is_none_or(|a| a != other.level as i64);

        let next_attempt = if has_changed {
            (fetch_time + hours(12)).timestamp()
        } else {
            match existing.last_changed {
                Some(x) if x + days(3).as_secs() as i64 > fetch_timestamp => {
                    (fetch_time + days(1)).timestamp()
                }
                Some(x) if x + days(7).as_secs() as i64 > fetch_timestamp => {
                    (fetch_time + days(3)).timestamp()
                }
                _ => (fetch_time + days(14)).timestamp(),
            }
        };

        let last_changed = existing
            .last_changed
            .filter(|_| !has_changed)
            .unwrap_or(fetch_timestamp);

        // Update the player with new info
        sqlx::query!(
            "UPDATE player
            SET level = ?, attributes = ?, next_report_attempt = ?,
                last_reported = ?, last_changed = ?, equip_count = ?, xp = ?
            WHERE player_id = ?",
            other.level,
            attributes,
            next_attempt,
            fetch_timestamp,
            last_changed,
            equip_count,
            experience,
            existing.player_id
        )
        .execute(&mut *tx)
        .await?;
        existing.player_id
    } else {
        let next_attempt = (fetch_time + days(1)).timestamp();
        // Insert a new player and so far unseen player. This is very unlikely
        // since players should be created after HoF search
        sqlx::query_scalar!(
            "INSERT INTO player
            (server_id, name, level, attributes, next_report_attempt, \
             last_reported, last_changed, equip_count, xp)
            VALUES (?,?,?,?,?,?,?,?,?)
            RETURNING player_id",
            server_id,
            player.name,
            other.level,
            attributes,
            next_attempt,
            fetch_timestamp,
            fetch_timestamp,
            equip_count,
            experience
        )
        .fetch_one(&mut *tx)
        .await?
    };

    let mut guild_id = None;
    if let Some(guild) = &player.guild {
        let guild_name = guild;
        let id = sqlx::query_scalar!(
            "INSERT INTO guild
            (server_id, name)
            VALUES (?, ?)
            ON CONFLICT(server_id, name) DO UPDATE SET is_removed = 0
            RETURNING guild_id",
            server_id,
            guild_name,
        )
        .fetch_one(&mut *tx)
        .await?;
        guild_id = Some(id);
    }

    let description = player.description.unwrap_or_default();
    let description_id = sqlx::query_scalar!(
        "INSERT INTO description (description) VALUES (?)
        ON CONFLICT(description)
        DO UPDATE SET description_id = description.description_id
        RETURNING description_id",
        description,
    )
    .fetch_one(&mut *tx)
    .await?;

    let response_id = sqlx::query_scalar!(
        "INSERT INTO otherplayer_resp (otherplayer_resp) VALUES (?)
        ON CONFLICT(otherplayer_resp)
        DO UPDATE SET otherplayer_resp_id = \
         otherplayer_resp.otherplayer_resp_id
        RETURNING otherplayer_resp_id",
        player.info,
    )
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query_scalar!(
        "INSERT INTO player_info (player_id, fetch_time, xp, level, \
         soldier_advice, description_id, guild_id, otherplayer_resp_id)
        VALUES (?,?,?,?,?,?,?,?)",
        pid,
        fetch_timestamp,
        experience,
        other.level,
        player.soldier_advice,
        description_id,
        guild_id,
        response_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!("DELETE FROM equipment WHERE player_id = ?", pid)
        .execute(&mut *tx)
        .await?;

    for ident in equip_idents {
        sqlx::query!(
            "INSERT INTO equipment (server_id, player_id, ident)
            VAlUES (?, ?, ?)",
            server_id,
            pid,
            ident
        )
        .execute(&mut *tx)
        .await?;
    }

    return Ok(tx.commit().await?);
}

#[derive(Debug, Deserialize)]
pub struct BugReportArgs {
    version: i32,
    os: String,
    arch: String,
    hwid: String,

    stacktrace: Option<String>,
    additional_info: Option<String>,
    error_text: Option<String>,
}

async fn report_bug(Json(args): Json<BugReportArgs>) -> Result<(), Response> {
    let current_time = Utc::now().to_rfc3339();
    sqlx::query!(
        "INSERT INTO error (stacktrace, version, additional_info, os, arch, \
         error_text, hwid, timestamp) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        args.stacktrace,
        args.version,
        args.additional_info,
        args.os,
        args.arch,
        args.error_text,
        args.hwid,
        current_time
    )
    .execute(&get_db().await?)
    .await
    .map_err(MFBotError::DBError)?;

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct GetCharactersArgs {
    server: String,
    limit: u32,
}

const fn minutes(minutes: u64) -> Duration {
    Duration::from_secs(60 * minutes)
}
const fn hours(hours: u64) -> Duration {
    Duration::from_secs(60 * 60 * hours)
}
const fn days(days: u64) -> Duration {
    Duration::from_secs(60 * 60 * 24 * days)
}

pub async fn get_characters_to_crawl(
    Json(args): Json<GetCharactersArgs>,
) -> Result<Json<Vec<String>>, Response> {
    let db = get_db().await?;
    let server_id = get_server_id(&db, args.server).await?;

    let now = Utc::now().timestamp();
    let next_retry = now + minutes(30).as_secs() as i64;

    let limit = args.limit.min(500);

    let todo = sqlx::query_scalar!(
        "WITH cte AS (
          SELECT rowid
          FROM player
          WHERE server_id = ?
            AND next_report_attempt < ?
            AND is_removed = false
          LIMIT ? )
        UPDATE player
        SET next_report_attempt = ?
        WHERE rowid IN (SELECT rowid FROM cte)
        RETURNING name",
        server_id,
        now,
        limit,
        next_retry
    )
    .fetch_all(&db)
    .await
    .map_err(MFBotError::DBError)?;

    Ok(Json(todo))
}

#[derive(Debug, Deserialize)]
pub struct GetHofArgs {
    server: String,
    player_count: usize,
    limit: u32,
}

pub async fn get_hof_pages_to_crawl(
    Json(args): Json<GetHofArgs>,
) -> Result<Json<Vec<i64>>, Response> {
    let db = get_db().await?;
    let server_id = get_server_id(&db, args.server).await?;

    let mut tx = db.begin().await.map_err(MFBotError::DBError)?;

    let now = Utc::now();
    let latest_accepted_crawling_start = (now - days(3)).timestamp();
    let now = now.timestamp();

    let last_hof_crawl = sqlx::query_scalar!(
        "WITH cte AS (
          SELECT rowid
          FROM server
          WHERE server_id = ? AND last_hof_crawl < ?
        )
        UPDATE server
        SET last_hof_crawl = ?
        WHERE rowid IN (SELECT rowid FROM cte)
        RETURNING server_id",
        server_id,
        latest_accepted_crawling_start,
        now
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(MFBotError::DBError)?;

    if last_hof_crawl.is_some() {
        // We restart HoF crawling
        sqlx::query!(
            "DELETE FROM todo_hof_page WHERE server_id = ?",
            server_id
        )
        .execute(&mut *tx)
        .await
        .map_err(MFBotError::DBError)?;

        let total_pages = (args.player_count as f32 / 51.0) as u32;

        sqlx::query!(
            "WITH RECURSIVE cnt(x) AS (
              SELECT 0
              UNION ALL
              SELECT x + 1 FROM cnt WHERE x < ?
            )
            INSERT INTO todo_hof_page (server_id, idx)
            SELECT ?, x FROM cnt;
        ",
            total_pages,
            server_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(MFBotError::DBError)?;
    }
    tx.commit().await.map_err(MFBotError::DBError)?;

    let limit = args.limit.min(100);
    let next_attempt_at = now + minutes(15).as_secs() as i64;

    let pages_to_crawl = sqlx::query_scalar!(
        "WITH cte AS (
          SELECT rowid
          FROM todo_hof_page
          WHERE server_id = ? AND next_report_attempt < ?
          LIMIT ?
        )
        UPDATE todo_hof_page
        SET next_report_attempt = ?
        WHERE rowid IN (SELECT rowid FROM cte)
        RETURNING idx",
        server_id,
        now,
        limit,
        next_attempt_at
    )
    .fetch_all(&db)
    .await
    .map_err(MFBotError::DBError)?;

    Ok(Json(pages_to_crawl))
}

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum MFBotError {
    #[error("DB Error: {0}")]
    DBError(#[from] sqlx::Error),
    #[error("Migrate Error: {0}")]
    MigrateError(#[from] sqlx::migrate::MigrateError),
    #[error("Invalid player reported: {0}")]
    InvalidPlayer(Box<str>),
    #[error("Invalid scrapbook")]
    InvalidScrapbook,
    #[error("Invalid server")]
    InvalidServer,
    #[error("Internal Server Error")]
    Internal,
}

impl From<MFBotError> for axum::response::Response {
    fn from(value: MFBotError) -> Self {
        axum::response::IntoResponse::into_response((
            StatusCode::INTERNAL_SERVER_ERROR,
            value.to_string(),
        ))
    }
}
