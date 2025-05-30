use std::time::Duration;

use axum::{
    Json, Router,
    http::{
        Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, post},
};
use chrono::Utc;
use db::{get_db, get_server_id};
use log::error;
use mfbot_server::*;
use sf_api::gamestate::{
    ServerTime,
    social::{HallOfFamePlayer, OtherPlayer},
    unlockables::{EquipmentIdent, ScrapBook},
};
use sqlx::QueryBuilder;
use tower_http::cors::{Any, CorsLayer};

pub mod db;

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    tracing_subscriber::fmt::init();

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
        .allow_origin(Any);

    let app = Router::new()
        .route("/", get(root))
        .route("/scrapbook_advice", post(scrapbook_advice))
        .route("/get_crawl_hof_pages", post(get_hof_pages_to_crawl))
        .route("/get_crawl_players", post(get_characters_to_crawl))
        .route("/report_players", post(report_players))
        .route("/report_hof", post(report_hof_pages))
        .route("/report", post(report_bug))
        .layer(cors);

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

pub async fn scrapbook_advice(
    Json(args): Json<ScrapBookAdviceArgs>,
) -> Result<Json<Vec<ScrapBookAdvice>>, Response> {
    let sb = ScrapBook::parse(&args.raw_scrapbook)
        .ok_or(MFBotError::InvalidScrapbook)?;
    let collected: Vec<i32> =
        sb.items.into_iter().map(compress_ident).collect();
    let db = get_db().await?;
    let server_id = get_server_id(&db, args.server).await?;

    let mut tx = db.begin().await.map_err(MFBotError::DBError)?;
    sqlx::query!("SET enable_hashjoin = off")
        .execute(&mut *tx)
        .await
        .map_err(MFBotError::DBError)?;

    let resp = sqlx::query!(
        "
        SELECT name as player_name, new_count
    FROM player
    NATURAL JOIN (
        SELECT player_id, count(*) as new_count
        FROM equipment
        WHERE server_id = $1 AND ident != ALL($2::integer[])
        GROUP BY player_id
    ) a
    WHERE level <= $3 AND attributes <= $4 AND is_removed = false
    ORDER BY new_count DESC, level ASC, attributes ASC
    LIMIT 25",
        server_id,
        collected.as_slice(),
        args.max_level as i32,
        args.max_attrs as i64
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(MFBotError::DBError)?;

    tx.commit().await.map_err(MFBotError::DBError)?;

    Ok(Json(
        resp.into_iter()
            .flat_map(|a| {
                Some(ScrapBookAdvice {
                    player_name: a.player_name,
                    new_count: a.new_count? as u32,
                })
            })
            .collect(),
    ))
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
    db: &sqlx::Pool<sqlx::Postgres>,
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
            .map(|a| a.to_utc().naive_utc())
    else {
        return Err(MFBotError::InvalidPlayer(
            format!("Could not parse fetch date: {}", player.fetch_date).into(),
        ));
    };
    let now = Utc::now().naive_utc();
    if fetch_time > now {
        fetch_time = now;
    }

    let experience = other.experience as i64;

    let mut equip_idents: Vec<_> = other
        .equipment
        .0
        .values()
        .flatten()
        .filter(|a| a.model_id < 100)
        .filter_map(|item| item.equipment_ident().map(compress_ident))
        .collect();

    // Assassins may have two swords, which can be identical
    equip_idents.sort();
    equip_idents.dedup();

    let equip_count = other.equipment.0.values().flatten().count() as i32;

    let attributes = other
        .base_attributes
        .values()
        .chain(other.bonus_attributes.values())
        .copied()
        .map(i64::from)
        .sum::<i64>();

    let mut tx = db.begin().await?;

    let existing = sqlx::query!(
        "SELECT player_id, level, attributes, last_reported, xp, last_changed
         FROM player
         WHERE server_id = $1 AND name = $2",
        server_id,
        player.name
    )
    .fetch_optional(&mut *tx)
    .await?;

    let pid = if let Some(existing) = existing {
        if existing.last_reported.is_some_and(|a| a >= fetch_time) {
            log::warn!("Discarded player update for {}", player.name);
            return Ok(());
        }
        let has_changed = existing.attributes.is_none_or(|a| a != attributes)
            || existing.xp.is_none_or(|a| a != experience)
            || existing.level.is_none_or(|a| a != other.level as i32);

        let next_attempt = if has_changed {
            fetch_time
                + hours(fastrand::u64(11..14))
                + minutes(fastrand::u64(0..=59))
        } else {
            match existing.last_changed {
                Some(x) if x + days(3) > fetch_time => {
                    fetch_time
                        + days(1)
                        + hours(fastrand::u64(0..12))
                        + minutes(fastrand::u64(0..=59))
                }
                Some(x) if x + days(7) > fetch_time => {
                    fetch_time
                        + days(fastrand::u64(2..=4))
                        + hours(fastrand::u64(0..23))
                        + minutes(fastrand::u64(0..=59))
                }
                _ => {
                    fetch_time
                        + days(fastrand::u64(10..=14))
                        + hours(fastrand::u64(0..=23))
                        + minutes(fastrand::u64(0..=59))
                }
            }
        };

        let last_changed = existing
            .last_changed
            .filter(|_| !has_changed)
            .unwrap_or(fetch_time);

        // Update the player with new info
        sqlx::query!(
            "UPDATE player
            SET level = $1, attributes = $2, next_report_attempt = $3,
                last_reported = $4, last_changed = $5, equip_count = $6, xp = \
             $7, honor = $8
            WHERE player_id = $9",
            other.level as i32,
            attributes,
            next_attempt,
            fetch_time,
            last_changed,
            equip_count as i32,
            experience,
            other.honor as i32,
            existing.player_id,
        )
        .execute(&mut *tx)
        .await?;
        existing.player_id
    } else {
        let next_attempt = fetch_time + days(1);
        // Insert a new player and so far unseen player. This is very unlikely
        // since players should be created after HoF search
        sqlx::query_scalar!(
            "INSERT INTO player
            (server_id, name, level, attributes, next_report_attempt, \
             last_reported, last_changed, equip_count, xp, honor)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            RETURNING player_id",
            server_id,
            player.name,
            other.level as i32,
            attributes,
            next_attempt,
            fetch_time,
            fetch_time,
            equip_count as i16,
            experience,
            other.honor as i32
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
            VALUES ($1, $2)
            ON CONFLICT(server_id, name) DO UPDATE SET is_removed = FALSE
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
        "INSERT INTO description (description) VALUES ($1)
        ON CONFLICT(description)
        DO UPDATE SET description_id = description.description_id
        RETURNING description_id",
        description,
    )
    .fetch_one(&mut *tx)
    .await?;

    use zstd::stream::encode_all;

    let resp = encode_all(player.info.as_bytes(), 3)
        .map_err(|_| MFBotError::Internal)?;

    let digest = md5::compute(&resp);
    let hash = format!("{:x}", digest);

    let response_id = sqlx::query_scalar!(
        "INSERT INTO otherplayer_resp (otherplayer_resp, hash) VALUES ($1, $2)
        ON CONFLICT(hash)
        DO UPDATE SET otherplayer_resp_id = \
         otherplayer_resp.otherplayer_resp_id
        RETURNING otherplayer_resp_id",
        resp,
        hash
    )
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query_scalar!(
        "INSERT INTO player_info (player_id, fetch_time, xp, level, \
         soldier_advice, description_id, guild_id, otherplayer_resp_id, honor)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        pid,
        fetch_time,
        experience,
        other.level as i32,
        player.soldier_advice,
        description_id,
        guild_id,
        response_id,
        other.honor as i32
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!("DELETE FROM equipment WHERE player_id = $1", pid)
        .execute(&mut *tx)
        .await?;

    for ident in equip_idents {
        sqlx::query!(
            "INSERT INTO equipment (server_id, player_id, ident)
            VAlUES ($1, $2, $3)",
            server_id,
            pid,
            ident
        )
        .execute(&mut *tx)
        .await?;
    }

    return Ok(tx.commit().await?);
}

async fn report_bug(Json(args): Json<BugReportArgs>) -> Result<(), Response> {
    let current_time = Utc::now().naive_utc();
    sqlx::query!(
        "INSERT INTO error (stacktrace, version, additional_info, os, arch, \
         error_text, hwid, timestamp) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
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

    let now = Utc::now().naive_utc();
    let next_retry = now + minutes(30);

    let limit = args.limit.min(500) as i64;

    let todo = sqlx::query_scalar!(
        "WITH cte AS (
          SELECT player_id
          FROM player
          WHERE server_id = $1
            AND next_report_attempt < $2
            AND is_removed = false
          LIMIT $3 )
        UPDATE player
        SET next_report_attempt = $4
        WHERE player_id IN (SELECT player_id FROM cte)
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

pub async fn get_hof_pages_to_crawl(
    Json(args): Json<GetHofArgs>,
) -> Result<Json<Vec<i32>>, Response> {
    let db = get_db().await?;
    let server_id = get_server_id(&db, args.server).await?;

    let mut tx = db.begin().await.map_err(MFBotError::DBError)?;

    let now = Utc::now().naive_utc();
    let latest_accepted_crawling_start = now - days(3);

    let last_hof_crawl = sqlx::query_scalar!(
        "WITH cte AS (
          SELECT server_id
          FROM server
          WHERE server_id = $1 AND last_hof_crawl < $2
        )
        UPDATE server
        SET last_hof_crawl = $3
        WHERE server_id IN (SELECT server_id FROM cte)
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
            "DELETE FROM todo_hof_page WHERE server_id = $1",
            server_id
        )
        .execute(&mut *tx)
        .await
        .map_err(MFBotError::DBError)?;

        let total_pages = (args.player_count as f32 / 51.0) as i32;

        sqlx::query!(
            "WITH RECURSIVE cnt(x) AS (
              SELECT 0
              UNION ALL
              SELECT x + 1 FROM cnt WHERE x < $1
            )
            INSERT INTO todo_hof_page (server_id, idx)
            SELECT $2, x FROM cnt;
        ",
            total_pages,
            server_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(MFBotError::DBError)?;
    }
    tx.commit().await.map_err(MFBotError::DBError)?;

    let limit = args.limit.min(100) as i64;
    let next_attempt_at = now + minutes(15);

    let pages_to_crawl = sqlx::query_scalar!(
        "WITH cte AS (
          SELECT idx
          FROM todo_hof_page
          WHERE server_id = $1 AND next_report_attempt < $2
          LIMIT $3
        )
        UPDATE todo_hof_page
        SET next_report_attempt = $4
        WHERE server_id = $1 AND idx IN (SELECT idx FROM cte)
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

pub async fn report_hof_pages(
    Json(args): Json<ReportHofArgs>,
) -> Result<(), Response> {
    let db = get_db().await?;
    let server_id = get_server_id(&db, args.server).await?;

    for (page, info) in args.pages {
        let mut tx = db.begin().await.map_err(MFBotError::DBError)?;
        let mut players = vec![];
        for player in info.as_str().trim_matches(';').split(';') {
            // Stop parsing once we receive an empty player
            if player.ends_with(",,,0,0,0,") {
                break;
            }
            match HallOfFamePlayer::parse(player) {
                Ok(x) => {
                    players.push(x);
                }
                Err(err) => log::warn!("{err}"),
            }
        }

        sqlx::query!(
            "DELETE FROM todo_hof_page
            WHERE server_id = $1 AND idx = $2",
            server_id,
            page as i32
        )
        .execute(&mut *tx)
        .await
        .map_err(MFBotError::DBError)?;

        if players.is_empty() {
            tx.commit().await.map_err(MFBotError::DBError)?;
            continue;
        }

        let mut b =
            QueryBuilder::new("INSERT INTO player (server_id, name, level) ");
        b.push_values(players, |mut b, player| {
            b.push_bind(server_id)
                .push_bind(player.name)
                .push_bind(player.level as i32);
        });
        b.push(" ON CONFLICT DO NOTHING");
        b.build()
            .execute(&mut *tx)
            .await
            .map_err(MFBotError::DBError)?;
        tx.commit().await.map_err(MFBotError::DBError)?;
    }
    Ok(())
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

// TODO: nude players
// SELECT name, level, ATTRIBUTES
// FROM player
// where equip_count < 3 AND is_removed = false and server_id = 1 and ATTRIBUTES
// < 9000 and attributes is not null ORDER BY LEVEL desc
// LIMIT 50;
