use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method};
use serde::Deserialize;

use crate::token::AUTH_TOKEN_TOGGL;

#[derive(Deserialize, Debug)]
pub struct TimeEntry {
    pub at: String,
    pub description: Option<String>, // API docs say entry with no description is null/None, but in practice it's an empty string, e.g. ""
    /// Duration in seconds
    pub duration: i64,
    pub id: u64,
    pub start: DateTime<FixedOffset>,
    pub stop: Option<DateTime<FixedOffset>>,
}

pub async fn get_time_entries(
    days: i64,
) -> Result<Vec<TimeEntry>, reqwest::Error> {
    let authorization_value = format!(
        "Basic {}",
        general_purpose::STANDARD.encode(format!("{}:api_token", AUTH_TOKEN_TOGGL))
    );

    // Calculate the Unix timestamp x days ago
    let x_days_ago = Utc::now() - Duration::days(days);
    let since_timestamp = x_days_ago.timestamp();

    // Create the HTTP client and make the request
    let client = Client::new();
    let url = format!(
        "https://api.track.toggl.com/api/v9/me/time_entries?since={}",
        since_timestamp
    );
    let response = client
        .request(Method::GET, url)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, authorization_value)
        .send()
        .await?;

    // Handle the JSON response as an array
    let time_entries: Vec<TimeEntry> = response.json().await?;

    Ok(time_entries)
}
