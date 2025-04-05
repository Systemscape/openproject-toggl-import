use std::env;

use crate::{COMMENT_SEPARATOR, TimeEntryRequest, token::*};
use anyhow::Result;
use tracing::{debug, info};

pub async fn submit_entry(item: TimeEntryRequest) -> Result<()> {
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

    let res = client
        .post(url)
        .basic_auth("apikey", Some(OPENPROJECT_API_KEY))
        .json(&item)
        .send()
        .await?;

    info!("Response: {}", res.text().await?);

    Ok(())
}

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

    info!("entries: {:#?}", entries);

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

/*
pub async fn get_current_user() -> Result<User, String> {
    let client = reqwest::Client::new();

    let res = client
        .get(format!("{BASE_URL}/api/users/me?fields=id,login"))
        .bearer_auth(AUTH_TOKEN_YOUTRACK)
        .send()
        .await;

    let res = res.unwrap().text().await.unwrap();
    debug!("get_current_user - got res: {:#?}", res);

    let user: User = serde_json::from_str(&res).unwrap();
    Ok(user)
}
    */

#[cfg(test)]
mod test {
    use test_log::test;

    use crate::{TimeEntryRequest, openproject};

    #[test(tokio::test)]
    async fn test_serde() {
        openproject::get_existing_toggl_ids("458").await.unwrap();
    }

    #[tokio::test]
    async fn test_upload() {
        let entry = TimeEntryRequest {
            links: crate::Links {
                work_package: crate::Link {
                    href: format!("/api/v3/work_packages/{}", 50),
                },
                activity: crate::Link {
                    href: format!("/api/v3/time_entries/activities/{}", 1),
                },
            },
            hours: format!("PT{}S", 30 * 60), // 30 mins, should show as 0.5h
            start_time: chrono::Utc::now(),
            stop_time: chrono::Utc::now(),
            comment: crate::Comment {
                raw: "Test - CAN BE DELETED".into(),
            },
            spent_on: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        };

        openproject::submit_entry(entry).await.unwrap();
    }
}
