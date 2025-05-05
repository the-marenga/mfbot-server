use std::sync::OnceLock;

use axum::{
    Json, Router,
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use chrono::Utc;
use serde::Deserialize;
use sqlx::{
    Pool, Sqlite,
    sqlite::{
        SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions,
        SqliteSynchronous,
    },
};

static DB: OnceLock<Pool<Sqlite>> = OnceLock::new();

#[allow(clippy::expect_used, clippy::missing_panics_doc)]
pub fn get_state() -> Pool<Sqlite> {
    DB.get()
        .expect("STATE always has to be initialize first")
        .clone()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let con_option = SqliteConnectOptions::new()
        .filename("mfbot.db")
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .optimize_on_close(true, Some(400))
        .create_if_missing(true);

    let db = SqlitePoolOptions::new()
        .connect_with(con_option)
        .await
        .unwrap();

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
    .await
    .unwrap();

    DB.get_or_init(move || db);

    let app = Router::new()
        .route("/", get(root))
        .route("/report", post(report_bug));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:4949")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> axum::response::Redirect {
    axum::response::Redirect::permanent("https://forum.mfbot.de/")
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
    .execute(&get_state())
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
