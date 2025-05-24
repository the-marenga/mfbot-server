use axum::{
    Json, Router,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use chrono::Utc;
use db::{get_db, init_db};
use serde::Deserialize;
use sf_api::gamestate::unlockables::EquipmentIdent;
pub mod db;

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    tracing_subscriber::fmt::init();

    init_db().await?;

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
    for player in players {
        log::info!("Players reported: {player:?}");

        sqlx::query(
            "INSERT INTO raw_player 
            (fetch_date, name, server, info, description, guild, \
             soldier_advice) 
            VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(player.fetch_date)
        .bind(player.name)
        .bind(player.server)
        .bind(player.info)
        .bind(player.description)
        .bind(player.guild)
        .bind(player.soldier_advice)
        .execute(&get_db())
        .await
        .map_err(MFBotError::DBError)?;
    }
    Ok(())
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
    log::info!("Bug reported: {args:?}");

    sqlx::query(
        "INSERT INTO error (stacktrace, version, additional_info, os, arch, \
         error_text, hwid, timestamp) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(args.stacktrace)
    .bind(args.version)
    .bind(args.additional_info)
    .bind(args.os)
    .bind(args.arch)
    .bind(args.error_text)
    .bind(args.hwid)
    .bind(Utc::now().to_rfc3339())
    .execute(&get_db())
    .await
    .map_err(MFBotError::DBError)?;

    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[allow(missing_docs)]
pub enum MFBotError {
    #[error("DB Error: {0}")]
    DBError(sqlx::Error),
}

impl From<MFBotError> for axum::response::Response {
    fn from(value: MFBotError) -> Self {
        axum::response::IntoResponse::into_response((
            StatusCode::INTERNAL_SERVER_ERROR,
            value.to_string(),
        ))
    }
}
