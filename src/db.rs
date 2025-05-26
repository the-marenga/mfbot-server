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
            .filename(env!("DATABASE_URL").split_once(":").unwrap().1)
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

        // sqlx::migrate!("./migrations").run(&pool).await?;

        Result::<sqlx::Pool<Sqlite>, MFBotError>::Ok(pool)
    })
    .await
    .cloned()
}

static LOOKUP_CACHE: LazyLock<RwLock<HashMap<String, i64>>> =
    LazyLock::new(|| RwLock::const_new(HashMap::new()));

pub async fn get_server_id(
    db: &Pool<Sqlite>,
    mut url: String,
) -> Result<i64, MFBotError> {
    if !url.starts_with("http") {
        url = format!("https://{url}");
    }
    let Ok(mut server) = url::Url::parse(&url) else {
        log::error!("Could not parse url: {}", url);
        return Err(MFBotError::InvalidServer);
    };
    if server.set_scheme("https").is_err() {
        log::error!("Could not set scheme: {server}");
        return Err(MFBotError::InvalidServer);
    }
    server.set_path("");
    let url = server.to_string();

    if let Some(id) = LOOKUP_CACHE.read().await.get(&url) {
        return Ok(*id);
    }

    let mut cache = LOOKUP_CACHE.write().await;
    if let Some(id) = cache.get(&url) {
        return Ok(*id);
    }
    let server_id = sqlx::query_scalar!(
        "INSERT INTO server (url)
        VALUES ($1)
        ON CONFLICT(url) DO UPDATE SET last_hof_crawl = server.last_hof_crawl
        RETURNING server_id",
        url
    )
    .fetch_one(db)
    .await
    .map_err(MFBotError::DBError)?;

    log::info!("Fed server cache with {url}");
    cache.insert(url.to_string(), server_id);
    Ok(server_id)
}
