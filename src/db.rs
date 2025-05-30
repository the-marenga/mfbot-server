use std::{collections::HashMap, sync::LazyLock, time::Duration};

use chrono::Utc;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};
use tokio::sync::RwLock;

use crate::{MFBotError, days};
pub async fn get_db() -> Result<Pool<Postgres>, MFBotError> {
    static DB: async_once_cell::OnceCell<sqlx::Pool<sqlx::Postgres>> =
        async_once_cell::OnceCell::new();

    Ok(DB
        .get_or_try_init(
            PgPoolOptions::new()
                .max_connections(500)
                .max_lifetime(Some(Duration::from_secs(60 * 3)))
                .min_connections(10)
                .acquire_timeout(Duration::from_secs(100))
                .connect(env!("DATABASE_URL")),
        )
        .await?
        .to_owned())
}

static LOOKUP_CACHE: LazyLock<RwLock<HashMap<String, i32>>> =
    LazyLock::new(|| RwLock::const_new(HashMap::new()));

pub async fn get_server_id(
    db: &Pool<Postgres>,
    mut url: String,
) -> Result<i32, MFBotError> {
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
    let time = (Utc::now() - days(30)).naive_utc();
    let server_id = sqlx::query_scalar!(
        "INSERT INTO server (url, last_hof_crawl)
        VALUES ($1, $2)
        ON CONFLICT(url) DO UPDATE SET last_hof_crawl = server.last_hof_crawl
        RETURNING server_id",
        url,
        time
    )
    .fetch_one(db)
    .await
    .map_err(MFBotError::DBError)?;

    log::info!("Fed server cache with {url}");
    cache.insert(url.to_string(), server_id);
    Ok(server_id)
}
