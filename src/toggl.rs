use base64::{Engine as _, engine::general_purpose};
use chrono::{DateTime, Duration, FixedOffset, Utc};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method};
use serde::Deserialize;

use crate::token::AUTH_TOKEN_TOGGL;

/// **Note** it seems like it is not straightforward to determine whether an entry has been deleted
/// so there might appear entries that are not shown on toggl anymore
#[derive(Deserialize, Debug)]
pub struct TimeEntry {
    /// Toggl entry ID
    pub id: u64,
    pub description: Option<String>, // API docs say entry with no description is null/None, but in practice it's an empty string, e.g. ""
    /// Duration in seconds
    pub duration: i64,
    pub start: DateTime<FixedOffset>,
    pub stop: Option<DateTime<FixedOffset>>,
}

/// [`TimeEntry`] with extracted WP ID and description
#[derive(Debug)]
pub struct ExtendedTimeEntry {
    pub toggl_time_entry: TimeEntry,
    pub work_package_id: String,
    pub description: String,
}

pub async fn get_time_entries(days: i64) -> Result<Vec<TimeEntry>, reqwest::Error> {
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
