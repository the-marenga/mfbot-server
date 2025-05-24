use axum::{
    Json, Router,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use chrono::Utc;
use db::get_db;
use serde::Deserialize;
use sf_api::gamestate::unlockables::EquipmentIdent;
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
    for player in players {
        log::info!("Players reported: {player:?}");

        let Ok(mut server) = url::Url::parse(&player.server) else {
            log::error!("Could not parse url: {}", player.server);
            continue;
        };
        if server.set_scheme("").is_err() {
            log::error!("Could not set scheme: {server}");
            continue;
        }
        server.set_path("");
        let server_url = server.to_string();

        sqlx::query!(
            "INSERT INTO raw_player 
            (fetch_date, name, server, info, description, guild, \
             soldier_advice) 
            VALUES (?, ?, ?, ?, ?, ?, ?)",
            player.fetch_date,
            player.name,
            server_url,
            player.info,
            player.description,
            player.guild,
            player.soldier_advice,
        )
        .execute(&get_db().await?)
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
}

impl From<MFBotError> for axum::response::Response {
    fn from(value: MFBotError) -> Self {
        axum::response::IntoResponse::into_response((
            StatusCode::INTERNAL_SERVER_ERROR,
            value.to_string(),
        ))
    }
}
