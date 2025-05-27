use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct GetHofArgs {
    pub server: String,
    pub player_count: usize,
    pub limit: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetCharactersArgs {
    pub server: String,
    pub limit: u32,
}
#[derive(Debug, Deserialize, Serialize)]
pub struct BugReportArgs {
    pub version: i32,
    pub os: String,
    pub arch: String,
    pub hwid: String,

    pub stacktrace: Option<String>,
    pub additional_info: Option<String>,
    pub error_text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RawOtherPlayer {
    pub name: String,
    pub server: String,
    pub info: String,
    pub description: Option<String>,
    pub guild: Option<String>,
    pub soldier_advice: Option<i64>,
    pub fetch_date: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScrapBookAdviceArgs {
    pub raw_scrapbook: String,
    pub server: String,
    pub max_level: u16,
    pub max_attrs: u64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "server", derive(sqlx::prelude::FromRow))]
pub struct ScrapBookAdvice {
    pub player_name: String,
    pub new_count: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReportHofArgs {
    pub server: String,
    // page => Ranklistplayer
    pub pages: HashMap<u32, String>,
}
