use anyhow::Result;
use chrono::{DateTime, Utc};
use dialoguer::{Confirm, theme::ColorfulTheme};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    process::exit,
};
use token::*;
use tracing::{debug, info};

// Extract token module
mod openproject;
mod toggl;
mod token;

// Regular expression for OpenProject work package IDs (e.g., [OP#123])
const REGEX_STRING: &str = r"(?i)^\[OP#(\d+)\](?: +(.*))*";

const COMMENT_SEPARATOR: &str = " - ";

#[derive(Debug)]
struct ExtendedTimeEntry {
    toggl_time_entry: toggl::TimeEntry,
    work_package_id: String,
    description: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct TimeEntryRequest {
    #[serde(rename = "_links")]
    links: Links,
    hours: String,
    #[serde(rename = "startTime")]
    start_time: DateTime<Utc>,
    #[serde(rename = "stopTime")]
    stop_time: DateTime<Utc>,
    comment: Comment,
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

#[tokio::main]
async fn main() -> Result<()> {
    // install global collector configured based on RUST_LOG env var.
    tracing_subscriber::fmt::init();

    let op_activity_id = env::var("OPENPROJECT_DEFAULT_ACTIVITY_ID")
        .unwrap_or(OPENPROJECT_DEFAULT_ACTIVITY_ID.to_string());

    // Get all toggl time entries
    let time_entries: Vec<toggl::TimeEntry> = toggl::get_time_entries(2).await?;
    info!("Time entries: {:#?}", time_entries);

    // Create a regex to extract the Work Package ID from the time entry
    let re = Regex::new(REGEX_STRING).unwrap();

    // Filter out entries with no stop time (= still running) and duration less than 1 minute
    let time_entries = time_entries
        .into_iter()
        .filter(|x| x.stop.is_some() && x.duration >= 60);

    // Filter time entries that match the regex and return iterator of ExtendedTimeEntry with that data
    let time_entries = time_entries.filter_map(|entry| {
        let description = entry.description.clone().unwrap_or_default();
        re.captures(&description).and_then(|x| {
            // Work Package ID is in the first capture
            let work_package_id = x.get(1)?.as_str().to_string();
            // Description text is everything that follows, i.e., the second capture.
            let entry_description = x.get(2).map_or(String::new(), |m| m.as_str().to_string());

            info!("wp id: {:#?}", work_package_id);

            Some(ExtendedTimeEntry {
                toggl_time_entry: entry,
                work_package_id,
                description: entry_description,
            })
        })
    });

    // Group time entries by work package ID
    let mut wp_time_entries_map: HashMap<String, Vec<ExtendedTimeEntry>> = HashMap::new();
    for entry in time_entries {
        debug!(
            "WP ID: {}, description: {}",
            entry.work_package_id, entry.description
        );
        wp_time_entries_map
            .entry(entry.work_package_id.clone())
            .or_default()
            .push(entry);
    }

    // The unique work package IDs from all toggl time entries correspond to the HashMap's keys.
    let unique_wp_ids = wp_time_entries_map.keys().cloned().collect::<HashSet<_>>();
    debug!("unique_wp_ids: {:#?}", &unique_wp_ids);

    // First, check for existing time entries to avoid duplicates
    info!("Checking for existing time entries in OpenProject...");

    // Create a HashSet to store existing Toggl IDs in OpenProject
    let mut existing_toggl_ids = Vec::new();

    // For each work package, fetch existing Toggl IDs for that workpackage
    // Note: Openproject allows fetching ALL time entries, but this quickly becomes a huge lot of data we don't need.
    for wp_id in &unique_wp_ids {
        let mut ids_from_wp = openproject::get_existing_toggl_ids(wp_id).await?;
        existing_toggl_ids.append(&mut ids_from_wp);
    }

    info!(
        "Found {} existing time entries with Toggl IDs",
        existing_toggl_ids.len()
    );

    // Collect all individual time entries to submit
    let mut entries_to_submit = Vec::new();

    for (_wp_id, entries) in wp_time_entries_map.iter() {
        for entry in entries {
            // Skip entries that already exist based on Toggl ID
            let toggl_id = entry.toggl_time_entry.id.to_string();
            if existing_toggl_ids.contains(&toggl_id) {
                info!(
                    "Skipping already submitted entry with Toggl ID: {}",
                    toggl_id
                );
                continue;
            }

            let comment = toggl_id + COMMENT_SEPARATOR + &entry.description;

            let links = Links {
                work_package: Link {
                    href: format!("/api/v3/work_packages/{}", entry.work_package_id),
                },
                activity: Link {
                    href: format!("/api/v3/time_entries/activities/{}", op_activity_id),
                },
            };

            // Convert seconds to hours, maintaining precision
            let duration = format!("PT{}S", entry.toggl_time_entry.duration);
            let date = entry.toggl_time_entry.start.format("%Y-%m-%d").to_string();

            entries_to_submit.push(TimeEntryRequest {
                links,
                hours: duration,
                start_time: entry.toggl_time_entry.start.to_utc(),
                stop_time: entry.toggl_time_entry.start.to_utc(),
                comment: Comment { raw: comment },
                spent_on: date,
            });
        }
    }

    if entries_to_submit.is_empty() {
        info!("No new time entries to submit.");
        return Ok(());
    }

    info!("entries_to_submit: {:#?}", entries_to_submit);

    // Ask for confirmation before submitting
    if !Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Do you want to submit {} time entries to OpenProject?",
            entries_to_submit.len()
        ))
        .interact()
        .unwrap()
    {
        info!("Aborted by user.");
        exit(1);
    }

    for entry in entries_to_submit {
        openproject::submit_entry(entry).await?;
    }

    info!("All time entries submitted successfully!");
    Ok(())
}

#[cfg(test)]
mod test {
    use crate::REGEX_STRING;
    use regex::Regex;

    #[test]
    fn test_regex() {
        let re = Regex::new(REGEX_STRING).unwrap();
        let caps = re.captures("[OP#123] My Description").unwrap();

        assert_eq!(caps.get(1).unwrap().as_str(), "123");
        assert_eq!(caps.get(2).unwrap().as_str(), "My Description");
    }
}
