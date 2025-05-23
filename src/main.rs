use std::{sync::OnceLock, u32};

use axum::{
    Json, Router,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use chrono::Utc;
use serde::Deserialize;
use sf_api::gamestate::unlockables::EquipmentIdent;
use sqlx::{
    Pool, Sqlite,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions,
        SqliteSynchronous,
    },
};

static DB: OnceLock<Pool<Sqlite>> = OnceLock::new();

#[allow(clippy::expect_used, clippy::missing_panics_doc)]
pub fn get_db() -> Pool<Sqlite> {
    DB.get()
        .expect("STATE always has to be initialize first")
        .clone()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    tracing_subscriber::fmt::init();

    let con_option = SqliteConnectOptions::new()
        .filename("mfbot.db")
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .auto_vacuum(sqlx::sqlite::SqliteAutoVacuum::Incremental)
        .foreign_keys(true)
        .optimize_on_close(true, Some(u32::MAX))
        .create_if_missing(true);

    let db = SqlitePoolOptions::new().connect_with(con_option).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS error (
            error_id INTEGER PRIMARY KEY AUTOINCREMENT,
            stacktrace TEXT,
            version INT,
            additional_info TEXT,
            os TEXT,
            arch TEXT,
            error_text TEXT,
            hwid TEXT,
            timestamp TEXT
    )",
    )
    .execute(&db)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS raw_player (
            raw_player_id INTEGER PRIMARY KEY AUTOINCREMENT,
            fetch_date TEXT NOT NULL,
            name TEXT NOT NULL,
            server TEXT NOT NULL,
            info TEXT NOT NULL,

            description TEXT,
            guild TEXT,
            soldier_advice INTEGER
    )",
    )
    .execute(&db)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS server (
            server_id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT NOT NULL
        )",
    )
    .execute(&db)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS server_url_idx on server (url)")
        .execute(&db)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS player (
            player_id INT NOT NULL,
            server_id INT NOT NULL REFERENCES server(server_id),

            name TEXT NOT NULL,
            last_online INT NOT NULL,
            last_updated INT NOT NULL,
            PRIMARY KEY (server_id, player_id)
        )",
    )
    .execute(&db)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS equipment (
            server_id INT NOT NULL,
            player_id INT NOT NULL,
            slot INT NOT NULL,

            ident INT,
            PRIMARY KEY (server_id, player_id, slot),
            FOREIGN KEY 
                (server_id, player_id) REFERENCES player(server_id, player_id)
        )",
    )
    .execute(&db)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS equipment_lookup_idx on equipment (
            server_id, ident, player_id
        )",
    )
    .execute(&db)
    .await?;

    DB.get_or_init(move || db);

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
