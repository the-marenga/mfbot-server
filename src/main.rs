use axum::{
    Json, Router,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use chrono::Utc;
use db::{get_db, get_server_id};
use log::error;
use serde::Deserialize;
use sf_api::gamestate::{
    ServerTime, social::OtherPlayer, unlockables::EquipmentIdent,
};
pub mod db;

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(root))
        .route("/updatePlayers", post(report_players))
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
    res |= (ident.class.map(|a| a as i64).unwrap_or(0)) << 28; // 28..32
    res as i32
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
    let server_id = get_server_id(db, &player.server).await;
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
    let Ok(fetch_time) =
        chrono::DateTime::parse_from_rfc3339(&player.fetch_date)
    else {
        return Err(MFBotError::InvalidPlayer(
            format!("Could not parse fetch date: {}", player.fetch_date).into(),
        ));
    };
    let pid = other.player_id;
    sqlx::query!(
        "INSERT INTO player
            (player_id, server_id, name)
            VALUES (?, ?, ?)
            ON CONFLICT(server_id, player_id) DO NOTHING",
        pid,
        server_id,
        player.name
    )
    .execute(db)
    .await?;

    let mut guild_id = None;
    if let Some(guild) = &player.guild {
        let guild_name = guild.trim().to_lowercase();
        let id = sqlx::query_scalar!(
            "INSERT INTO guild
            (server_id, name)
            VALUES (?, ?)
            ON CONFLICT(server_id, name) DO UPDATE SET is_removed = 0
            RETURNING guild_id",
            server_id,
            guild_name,
        )
        .fetch_one(db)
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
    .fetch_one(db)
    .await?;

    let response_id = sqlx::query_scalar!(
        "INSERT INTO otherplayer_resp (otherplayer_resp) VALUES (?)
        ON CONFLICT(otherplayer_resp)
        DO UPDATE SET otherplayer_resp_id = \
         otherplayer_resp.otherplayer_resp_id
        RETURNING otherplayer_resp_id",
        player.info,
    )
    .fetch_one(db)
    .await?;

    let fetch_time = fetch_time.to_utc().timestamp();
    let experience = other.experience as i64;
    let info_id = sqlx::query_scalar!(
        "INSERT INTO player_info (player_id, server_id, fetch_time, xp, \
         level, soldier_advice, description_id, guild_id, otherplayer_resp_id)
        VALUES (?,?,?,?,?,?,?,?,?)
        RETURNING player_info_id",
        pid,
        server_id,
        fetch_time,
        experience,
        other.level,
        player.soldier_advice,
        description_id,
        guild_id,
        response_id
    )
    .fetch_one(db)
    .await?
    .ok_or(MFBotError::Internal)?;

    let newest_info_id = sqlx::query_scalar!(
        "SELECT player_info_id
        FROM player_info
        WHERE player_id = ? AND server_id = ?
        ORDER BY fetch_time desc
        LIMIT 1",
        pid,
        server_id
    )
    .fetch_one(db)
    .await?
    .ok_or(MFBotError::Internal)?;

    if newest_info_id != info_id {
        return Ok(());
    }
    // The info we received is the most up to date

    let equip_idents = other.equipment.0.values().filter_map(|item| {
        Some(compress_ident(item.as_ref()?.equipment_ident()?))
    });

    let mut tx = db.begin().await?;
    sqlx::query!(
        "DELETE FROM equipment WHERE server_id = ? AND player_id = ?",
        server_id,
        pid
    )
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

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum MFBotError {
    #[error("DB Error: {0}")]
    DBError(#[from] sqlx::Error),
    #[error("Migrate Error: {0}")]
    MigrateError(#[from] sqlx::migrate::MigrateError),
    #[error("Invalid player reported: {0}")]
    InvalidPlayer(Box<str>),
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
