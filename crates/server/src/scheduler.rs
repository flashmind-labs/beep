use crate::app::{App, Message};
use crate::store::CreateRun;
use chrono::{Datelike, Timelike, Utc};
use std::sync::Arc;

/// Minimal 5-field cron matcher: minute hour day-of-month month day-of-week
/// Supports: *, specific numbers, comma-separated lists, ranges (1-5), steps (*/5)
fn cron_matches(expression: &str, now: &chrono::DateTime<chrono::Utc>) -> bool {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    let values = [
        now.minute(),
        now.hour(),
        now.day(),
        now.month(),
        now.weekday().num_days_from_sunday(),
    ];
    fields
        .iter()
        .zip(values.iter())
        .all(|(field, &value)| field_matches(field, value))
}

fn field_matches(field: &str, value: u32) -> bool {
    if field == "*" {
        return true;
    }
    field.split(',').any(|part| {
        if let Some((range, step)) = part.split_once('/') {
            let step: u32 = step.parse().unwrap_or(1);
            if step == 0 {
                return false;
            }
            if range == "*" {
                return value % step == 0;
            }
            if let Some((lo, hi)) = range.split_once('-') {
                let lo: u32 = lo.parse().unwrap_or(0);
                let hi: u32 = hi.parse().unwrap_or(0);
                return value >= lo && value <= hi && (value - lo) % step == 0;
            }
            return false;
        }
        if let Some((lo, hi)) = part.split_once('-') {
            let lo: u32 = lo.parse().unwrap_or(0);
            let hi: u32 = hi.parse().unwrap_or(0);
            return value >= lo && value <= hi;
        }
        part.parse::<u32>().ok() == Some(value)
    })
}

pub async fn run(app: Arc<App>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        if let Err(e) = tick(&app).await {
            tracing::error!(%e, "cron scheduler tick failed");
        }
    }
}

async fn tick(app: &Arc<App>) -> anyhow::Result<()> {
    let now = Utc::now();
    let crons = app.store.due_crons().await?;
    for (user_id, cron) in crons {
        let id = cron["id"].as_str().unwrap_or_default();
        let schedule = cron["schedule"].as_str().unwrap_or_default();
        let prompt = cron["prompt"].as_str().unwrap_or_default();
        let label = cron["label"].as_str().unwrap_or_default();

        if !cron_matches(schedule, &now) {
            continue;
        }

        // Check if already ran this minute
        if let Ok(Some(last)) = app.store.cron_last_run(&user_id, id).await {
            if let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(&last) {
                if now.signed_duration_since(last_time).num_seconds() < 55 {
                    continue;
                }
            }
            // Also try the SQLite datetime() format
            if let Ok(last_time) =
                chrono::NaiveDateTime::parse_from_str(&last, "%Y-%m-%d %H:%M:%S")
            {
                let last_utc = last_time.and_utc();
                if now.signed_duration_since(last_utc).num_seconds() < 55 {
                    continue;
                }
            }
        }

        tracing::info!(user=%user_id, cron_id=%id, label, "firing cron");

        let session_id = uuid::Uuid::new_v4();
        let request_id = uuid::Uuid::new_v4();
        let message = format!("[Scheduled: {}] {}", label, prompt);
        let msg = Message {
            session_id,
            request_id,
            message,
        };
        match app.submit(&user_id, msg).await {
            Ok(CreateRun::Created(run)) => {
                let _ = app.store.log_cron_start(&user_id, id, &run.id).await;
            }
            Ok(_) => tracing::warn!(cron_id=%id, "cron run conflicted"),
            Err(e) => tracing::error!(cron_id=%id, %e, "failed to submit cron run"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_cron_matches_every_minute() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 30, 0).unwrap();
        assert!(cron_matches("* * * * *", &now));
    }

    #[test]
    fn test_cron_matches_specific_minute() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 30, 0).unwrap();
        assert!(cron_matches("30 * * * *", &now));
        assert!(!cron_matches("31 * * * *", &now));
    }

    #[test]
    fn test_cron_matches_step() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 15, 0).unwrap();
        assert!(cron_matches("*/5 * * * *", &now));
        assert!(cron_matches("*/15 * * * *", &now));
        assert!(!cron_matches("*/7 * * * *", &now));
    }

    #[test]
    fn test_cron_matches_range() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 14, 0, 0).unwrap();
        assert!(cron_matches("0 9-17 * * *", &now));
        assert!(!cron_matches("0 9-13 * * *", &now));
    }

    #[test]
    fn test_cron_matches_comma_list() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert!(cron_matches("0 8,12,18 * * *", &now));
        assert!(!cron_matches("0 8,13,18 * * *", &now));
    }

    #[test]
    fn test_invalid_expression() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert!(!cron_matches("not a cron", &now));
        assert!(!cron_matches("* * *", &now));
    }
}
