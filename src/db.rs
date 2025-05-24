use std::sync::OnceLock;

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

pub async fn init_db() -> Result<(), sqlx::Error> {
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

    Ok(())
}
