use std::env;

use crate::{COMMENT_SEPARATOR, toggl::ExtendedTimeEntry, token::*};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Serialize, Deserialize, Debug)]
pub struct TimeEntryRequest {
    #[serde(rename = "_links")]
    pub links: Links,
    pub hours: String,
    #[serde(rename = "startTime")]
    pub start_time: DateTime<Utc>,
    #[serde(rename = "stopTime")]
    pub stop_time: DateTime<Utc>,
    pub comment: Comment,
    #[serde(rename = "spentOn")]
    pub spent_on: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Links {
    #[serde(rename = "workPackage")]
    work_package: Link,
    activity: Link,
}

#[derive(Serialize, Deserialize, Debug)]
struct Link {
    href: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Comment {
    raw: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct User {
    pub id: String,
}

impl TimeEntryRequest {
    pub fn from(entry: &ExtendedTimeEntry, activity_id: &str) -> Self {
        let links = Links::from(&entry.work_package_id, activity_id);
        let toggl_id = entry.toggl_time_entry.id.to_string();
        let duration = format!("PT{}S", entry.toggl_time_entry.duration);
        let date = entry.toggl_time_entry.start.format("%Y-%m-%d").to_string();
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

    pub async fn upload(&self) -> Result<()> {
        // Get environment variables
        let op_host = env::var("OPENPROJECT_HOST").unwrap_or(OPENPROJECT_HOST.to_string());
        let op_http_schema =
            env::var("OPENPROJECT_HTTP_SCHEMA").unwrap_or(OPENPROJECT_HTTP_SCHEMA.to_string());

        // Base URL for OpenProject API
        let op_base_url = format!("{}://{}/api/v3/", op_http_schema, op_host);
        info!("OpenProject base URL: {}", op_base_url);

        let client = reqwest::Client::new();
        // Base URL for OpenProject API
        let url = format!("{}time_entries", op_base_url);

        let res: reqwest::Response = client
            .post(url)
            .basic_auth("apikey", Some(OPENPROJECT_API_KEY))
            .json(self)
            .send()
            .await?;

        info!("Response: {}", res.text().await?);

        Ok(())
    }
}

impl Links {
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
    pub fn from(comment: String) -> Self {
        Self { raw: comment }
    }
}

/// Get a list of already existing toggl IDs for a workpackage
pub async fn get_existing_toggl_ids(wp_id: &str) -> Result<Vec<String>> {
    debug!("get_workitems for issue_id {}", wp_id);
    let uri = format!(
        "time_entries?pageSize=100&filters=[{{\"work_package\":{{\"operator\":\"=\",\"values\":[\"{wp_id}\"]}}}}]"
    );

    let res = perform_request(&uri).await.unwrap();
    let res = res.error_for_status()?;

    let entries: serde_json::Value = match res.json().await {
        Ok(json) => json,
        Err(e) => {
            return Err(anyhow::Error::msg(format!(
                "Error parsing time entries JSON for WP #{}: {}",
                wp_id, e,
            )));
        }
    };

    // Create a HashSet to store existing Toggl IDs in OpenProject
    let mut existing_toggl_ids = Vec::new();

    // Extract Toggl IDs from the custom field
    if let Some(elements_array) = entries
        .get("_embedded")
        .and_then(|e| e.get("elements"))
        .and_then(|e| e.as_array())
    {
        for element in elements_array {
            if let Some(comment_field) = element.get("comment").and_then(|c| c.get("raw")) {
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

pub async fn perform_request(uri: &str) -> Result<reqwest::Response, reqwest::Error> {
    // Get environment variables
    let op_host = env::var("OPENPROJECT_HOST").unwrap_or(OPENPROJECT_HOST.to_string());
    let op_http_schema =
        env::var("OPENPROJECT_HTTP_SCHEMA").unwrap_or(OPENPROJECT_HTTP_SCHEMA.to_string());

    // Base URL for OpenProject API
    let op_base_url = format!("{}://{}/api/v3/", op_http_schema, op_host);
    info!("OpenProject base URL: {}", op_base_url);

    let client = reqwest::Client::new();

    client
        .get(op_base_url + uri)
        .basic_auth("apikey", Some(OPENPROJECT_API_KEY))
        .send()
        .await
}

#[cfg(test)]
mod test {
    use test_log::test;

    use crate::openproject::{self, Comment, Link, Links, TimeEntryRequest};

    #[test(tokio::test)]
    async fn test_serde() {
        openproject::get_existing_toggl_ids("458").await.unwrap();
    }

    #[tokio::test]
    async fn test_upload() {
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

        entry.upload().await.unwrap();
    }
}
