use std::env;

use crate::{COMMENT_SEPARATOR, toggl::ExtendedTimeEntry, token::*};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use reqwest::{Client, Response};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Reusable client that stores endpoint and credentials
pub struct OpenProjectClient {
    client: Client,
    base_url: String,
    apikey: SecretString,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TimeEntryRequest {
    /// Links to workpackage and activity
    #[serde(rename = "_links")]
    links: Links,
    /// Time as ISO 8601 duration (can also be seconds or minutes!)
    hours: String,
    /// Entry start time
    #[serde(rename = "startTime")]
    start_time: DateTime<Utc>,
    /// Entry end time
    #[serde(rename = "stopTime")]
    stop_time: DateTime<Utc>,
    /// Comment/Description
    comment: Comment,
    /// Day at which the time has been spent formatted as YYYY-MM-DD
    #[serde(rename = "spentOn")]
    spent_on: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Links {
    #[serde(rename = "workPackage")]
    work_package: Link,
    activity: Link,
}

#[derive(Serialize, Deserialize, Debug)]
struct Link {
    href: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Comment {
    raw: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct User {
    pub id: String,
}

impl OpenProjectClient {
    pub fn new() -> Self {
        // Get environment variables
        let op_host = env::var("OPENPROJECT_HOST").unwrap_or(OPENPROJECT_HOST.to_string());
        let op_http_schema =
            env::var("OPENPROJECT_HTTP_SCHEMA").unwrap_or(OPENPROJECT_HTTP_SCHEMA.to_string());

        let op_apikey = env::var("OPENPROJECT_API_KEY").unwrap_or(OPENPROJECT_API_KEY.to_string());

        // Base URL for OpenProject API
        let base_url = format!("{}://{}/api/v3/", op_http_schema, op_host);
        info!("OpenProject base URL: {}", base_url);

        let client = reqwest::Client::new();
        Self {
            client,
            base_url,
            apikey: SecretString::from(op_apikey),
        }
    }

    /// Path without leading slash, payload is posted as JSON
    async fn post(&self, path: &str, payload: impl Serialize) -> Result<Response> {
        // Base URL for OpenProject API + a path
        let url = format!("{}{}", self.base_url, path);

        self.client
            .post(url)
            .basic_auth("apikey", Some(self.apikey.expose_secret()))
            .json(&payload)
            .send()
            .await
            .context("POST request failed")
    }

    async fn get(&self, path: &str) -> Result<Response> {
        // Base URL for OpenProject API + a path
        let url = format!("{}{}", self.base_url, path);

        self.client
            .get(url)
            .basic_auth("apikey", Some(self.apikey.expose_secret()))
            .send()
            .await
            .context("GET request failed")
    }
}

impl TimeEntryRequest {
    pub fn from(entry: &ExtendedTimeEntry, activity_id: &str) -> Self {
        let links = Links::from(&entry.work_package_id, activity_id);
        let toggl_id = entry.toggl_time_entry.id.to_string();
        let duration = format!("PT{}S", entry.toggl_time_entry.duration);
        let date = entry.toggl_time_entry.start.format("%Y-%m-%d").to_string();

        // Comment string containing the toggl ID to check whether it has been uploaded already
        let comment = toggl_id + COMMENT_SEPARATOR + &entry.description;

        Self {
            links,
            hours: duration,
            start_time: entry.toggl_time_entry.start.to_utc(),
            stop_time: entry.toggl_time_entry.start.to_utc(),
            comment: Comment::from(comment),
            spent_on: date,
        }
    }

    /// Upload a time TimeEntry to openproject
    pub async fn upload(&self, op_client: &OpenProjectClient) -> Result<()> {
        let res = op_client.post("time_entries", self).await?;

        info!("Response: {}", res.text().await?);

        Ok(())
    }
}

impl Links {
    /// Helper function to create a valid Links struct from a workpackage ID and an activity ID
    pub fn from(wp_id: &str, activity_id: &str) -> Self {
        Self {
            work_package: Link {
                href: format!("/api/v3/work_packages/{}", wp_id),
            },
            activity: Link {
                href: format!("/api/v3/time_entries/activities/{}", activity_id),
            },
        }
    }
}

impl Comment {
    /// Helper function to fill the Comment (wrapper) struct
    pub fn from(comment: String) -> Self {
        Self { raw: comment }
    }
}

/// Get a list of already existing toggl IDs for a workpackage
pub async fn get_existing_toggl_ids(
    op_client: &OpenProjectClient,
    wp_id: &str,
) -> Result<Vec<String>> {
    debug!("get_workitems for issue_id {}", wp_id);

    // Construct a query that gets time entries filtered for the given workpackage ID
    let uri = format!(
        "time_entries?pageSize=100&filters=[{{\"work_package\":{{\"operator\":\"=\",\"values\":[\"{wp_id}\"]}}}}]"
    );

    let res = op_client.get(&uri).await?;
    let res = res.error_for_status()?;

    let entries: serde_json::Value = res.json().await.map_err(|e| {
        anyhow::Error::msg(format!(
            "Error parsing time entries JSON for WP #{}: {}",
            wp_id, e,
        ))
    })?;

    // Create a HashSet to store existing Toggl IDs in OpenProject
    let mut existing_toggl_ids = Vec::new();

    // Extract Toggl IDs from the custom field which is nested under the
    // `_embedded` and `element` tags
    if let Some(elements_array) = entries
        .get("_embedded")
        .and_then(|e| e.get("elements"))
        .and_then(|e| e.as_array())
    {
        for element in elements_array {
            if let Some(comment_field) = element.get("comment").and_then(|c| c.get("raw")) {
                // Extract the toggl ID as the first part before the comment separator
                existing_toggl_ids.push(
                    comment_field
                        .as_str()
                        .unwrap()
                        .split_once(COMMENT_SEPARATOR)
                        .unwrap()
                        .0
                        .to_string(),
                );
            }
        }
    }
    debug!("Got existing_toggl_ids: {:#?}", existing_toggl_ids);
    Ok(existing_toggl_ids)
}

#[cfg(test)]
mod test {
    use test_log::test;

    use crate::openproject::{self, Comment, Link, Links, OpenProjectClient, TimeEntryRequest};

    #[test(tokio::test)]
    async fn test_serde() {
        let op_client = OpenProjectClient::new();
        openproject::get_existing_toggl_ids(&op_client, "458")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_upload() {
        let op_client = OpenProjectClient::new();

        let entry = TimeEntryRequest {
            links: Links {
                work_package: Link {
                    href: format!("/api/v3/work_packages/{}", 50),
                },
                activity: Link {
                    href: format!("/api/v3/time_entries/activities/{}", 1),
                },
            },
            hours: format!("PT{}S", 30 * 60), // 30 mins, should show as 0.5h
            start_time: chrono::Utc::now(),
            stop_time: chrono::Utc::now(),
            comment: Comment {
                raw: "Test - CAN BE DELETED".into(),
            },
            spent_on: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        };

        entry.upload(&op_client).await.unwrap();
    }
}
