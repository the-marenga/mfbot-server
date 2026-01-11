use std::sync::Arc;

use axum::{
    Json, Router,
    http::{
        Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, post},
};
use sf_info_lib::{
    db::{get_characters_to_crawl, underworld::get_best_nude_players, *},
    error::SFSError,
    types::*,
};
#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;
use tower_http::cors::{Any, CorsLayer};

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn core::error::Error>> {
    tracing_subscriber::fmt::init();

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION])
        .allow_origin(Any);

    let app = Router::new()
        .route("/", get(root))
        .route("/scrapbook_advice", post(scrapbook_advice))
        .route("/underworld_advice", post(underworld_advice))
        .route("/get_crawl_hof_pages", post(get_crawl_hof_pages))
        .route("/get_crawl_players", post(get_crawl_chars))
        .route("/report_players", post(report_players))
        .route("/report_hof", post(report_hof_pages))
        .route("/report", post(report_bug))
        .layer(cors);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:4949").await?;
    Ok(axum::serve(listener, app).await?)
}

async fn root() -> axum::response::Redirect {
    axum::response::Redirect::permanent("https://forum.mfbot.de/")
}

async fn report_players(
    Json(report): Json<CrawlReport>,
) -> Result<(), Response> {
    handle_crawl_report(report).await.map_err(to_response)
}

async fn get_crawl_hof_pages(
    Json(args): Json<GetHofArgs>,
) -> Result<Json<Vec<i32>>, Response> {
    get_hof_pages_to_crawl(args)
        .await
        .map_err(to_response)
        .map(Json)
}

async fn report_hof_pages(
    Json(args): Json<ReportHofArgs>,
) -> Result<(), Response> {
    insert_hof_pages(args).await.map_err(to_response)
}

async fn get_crawl_chars(
    Json(args): Json<GetCharactersArgs>,
) -> Result<Json<Vec<String>>, Response> {
    get_characters_to_crawl(args)
        .await
        .map_err(to_response)
        .map(Json)
}

async fn scrapbook_advice(
    Json(args): Json<ScrapBookAdviceArgs>,
) -> Result<Json<Arc<[ScrapBookAdvice]>>, Response> {
    get_scrapbook_advice(args)
        .await
        .map_err(to_response)
        .map(Json)
}

async fn underworld_advice(
    Json(args): Json<UnderworldAdviceArgs>,
) -> Result<Json<Arc<[UnderworldAdvice]>>, Response> {
    get_best_nude_players(args)
        .await
        .map_err(to_response)
        .map(Json)
}

async fn report_bug(Json(args): Json<BugReportArgs>) -> Result<(), Response> {
    insert_bug(args).await.map_err(to_response)
}

pub fn to_response(value: SFSError) -> axum::response::Response {
    axum::response::IntoResponse::into_response((
        StatusCode::INTERNAL_SERVER_ERROR,
        value.to_string(),
    ))
}
