use log::error;
use sqlx::{Sqlite, sqlite::*};

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
