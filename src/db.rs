use std::{collections::HashMap, sync::LazyLock};

use log::error;
use sqlx::{Pool, Sqlite, sqlite::*};
use tokio::sync::RwLock;

use crate::MFBotError;

pub async fn get_db() -> Result<sqlx::Pool<Sqlite>, MFBotError> {
    use async_once_cell::OnceCell;
    static DB: OnceCell<sqlx::Pool<Sqlite>> = OnceCell::new();
    DB.get_or_try_init(async {
        let options = SqliteConnectOptions::new()
            .filename(env!("DATABASE_URL"))
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .auto_vacuum(sqlx::sqlite::SqliteAutoVacuum::Incremental)
            .foreign_keys(true)
            .optimize_on_close(true, Some(u32::MAX))
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .connect_with(options)
            .await
            .inspect_err(|e| {
                error!("Database connection error: {:?}", e);
            })?;

        sqlx::migrate!("./migrations").run(&pool).await?;

        Result::<sqlx::Pool<Sqlite>, MFBotError>::Ok(pool)
    })
    .await
    .cloned()
}

pub async fn get_server_id(db: &Pool<Sqlite>, url: &str) -> Option<i64> {
    let Ok(mut server) = url::Url::parse(url) else {
        log::error!("Could not parse url: {}", url);
        return None;
    };
    if server.set_scheme("https").is_err() {
        log::error!("Could not set scheme: {server}");
        return None;
    }
    server.set_path("");
    let url = server.to_string();

    static LOOKUP_CACHE: LazyLock<RwLock<HashMap<String, i64>>> =
        LazyLock::new(|| RwLock::const_new(HashMap::new()));
    if let Some(id) = LOOKUP_CACHE.read().await.get(&url) {
        return Some(*id);
    }

    let mut cache = LOOKUP_CACHE.write().await;
    if let Some(id) = cache.get(&url) {
        return Some(*id);
    }
    let server_id = sqlx::query_scalar!(
        "INSERT INTO server (url)
        VALUES ($1)
        ON CONFLICT(url) DO UPDATE SET url = excluded.url
        RETURNING server_id",
        url
    )
    .fetch_one(db)
    .await
    .map_err(MFBotError::DBError)
    .ok()?;

    cache.insert(url.to_string(), server_id);
    Some(server_id)
}
