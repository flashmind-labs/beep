use crate::app::{App, Message};
use crate::store::CreateRun;
use chrono::{Datelike, TimeZone, Timelike, Utc};
use cron::Schedule;
use std::str::FromStr;
use std::sync::Arc;

/// Check if a 5-field POSIX cron expression matches the current minute in a given timezone.
/// Converts to the 7-field format (sec min hour dom month dow year) expected by the `cron` crate.
fn cron_matches_tz(expression: &str, now: &chrono::DateTime<chrono::Utc>, timezone: &str) -> bool {
    let fields: Vec<&str> = expression.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }
    let seven_field = format!("0 {} *", expression);
    let schedule = match Schedule::from_str(&seven_field) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // Convert UTC now to the user's timezone for matching
    let local_now = if let Ok(tz) = timezone.parse::<chrono_tz::Tz>() {
        now.with_timezone(&tz)
    } else {
        // Fallback to UTC if timezone is invalid
        let utc_tz = chrono_tz::UTC;
        now.with_timezone(&utc_tz)
    };
    // Build a UTC DateTime with the local time values so cron matching works in local time
    let fake_utc = Utc
        .with_ymd_and_hms(
            local_now.year(),
            local_now.month(),
            local_now.day(),
            local_now.hour(),
            local_now.minute(),
            local_now.second(),
        )
        .single();
    let Some(fake_utc) = fake_utc else {
        return false;
    };
    let window_start = fake_utc - chrono::Duration::seconds(30);
    if let Some(next) = schedule.after(&window_start).next() {
        next.minute() == fake_utc.minute()
            && next.hour() == fake_utc.hour()
            && next.day() == fake_utc.day()
            && next.month() == fake_utc.month()
            && next.year() == fake_utc.year()
    } else {
        false
    }
}

pub async fn run(app: Arc<App>) {
    let mut tick_count: u64 = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        if let Err(e) = tick(&app).await {
            tracing::error!(%e, "cron scheduler tick failed");
        }
        // Purge old notifications roughly once per hour (120 ticks * 30s = 3600s)
        tick_count += 1;
        if tick_count.is_multiple_of(120) {
            match app.store.purge_old_notifications().await {
                Ok(n) if n > 0 => tracing::info!(deleted = n, "purged old notifications"),
                Err(e) => tracing::warn!(%e, "notification purge failed"),
                _ => {}
            }
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
        let run_once = cron["run_once"].as_bool().unwrap_or(false);

        // Use the user's timezone for cron matching
        let tz = app
            .store
            .get_timezone(&user_id)
            .await
            .unwrap_or_else(|_| "UTC".into());
        if !cron_matches_tz(schedule, &now, &tz) {
            continue;
        }

        // Check if already ran this minute
        if let Ok(Some(last)) = app.store.cron_last_run(&user_id, id).await {
            if let Ok(last_time) = chrono::DateTime::parse_from_rfc3339(&last)
                && now.signed_duration_since(last_time).num_seconds() < 55
            {
                continue;
            }
            if let Ok(last_time) = chrono::NaiveDateTime::parse_from_str(&last, "%Y-%m-%d %H:%M:%S")
            {
                let last_utc = last_time.and_utc();
                if now.signed_duration_since(last_utc).num_seconds() < 55 {
                    continue;
                }
            }
        }

        tracing::info!(user=%user_id, cron_id=%id, label, run_once, "firing cron");

        let session_id = uuid::Uuid::new_v4();
        let request_id = uuid::Uuid::new_v4();
        let message = format!("[Scheduled: {}] {}", label, prompt);
        let msg = Message {
            session_id,
            request_id,
            message,
            cron_id: Some(id.to_owned()),
        };
        match app.submit(&user_id, msg).await {
            Ok(CreateRun::Created(run)) => {
                let _ = app.store.log_cron_start(&user_id, id, &run.id).await;
                // Auto-disable one-time schedules after firing
                if run_once {
                    let _ = app
                        .store
                        .update_cron(&user_id, id, Some(false), None, None)
                        .await;
                    tracing::info!(cron_id=%id, "one-time cron disabled after firing");
                }
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
        assert!(cron_matches_tz("* * * * *", &now, "UTC"));
    }

    #[test]
    fn test_cron_matches_specific_minute() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 30, 0).unwrap();
        assert!(cron_matches_tz("30 * * * *", &now, "UTC"));
        assert!(!cron_matches_tz("31 * * * *", &now, "UTC"));
    }

    #[test]
    fn test_cron_matches_step() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 15, 0).unwrap();
        assert!(cron_matches_tz("*/5 * * * *", &now, "UTC"));
        assert!(cron_matches_tz("*/15 * * * *", &now, "UTC"));
        assert!(!cron_matches_tz("*/7 * * * *", &now, "UTC"));
    }

    #[test]
    fn test_cron_matches_range() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 14, 0, 0).unwrap();
        assert!(cron_matches_tz("0 9-17 * * *", &now, "UTC"));
        assert!(!cron_matches_tz("0 9-13 * * *", &now, "UTC"));
    }

    #[test]
    fn test_cron_matches_comma_list() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert!(cron_matches_tz("0 8,12,18 * * *", &now, "UTC"));
        assert!(!cron_matches_tz("0 8,13,18 * * *", &now, "UTC"));
    }

    #[test]
    fn test_invalid_expression() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert!(!cron_matches_tz("not a cron", &now, "UTC"));
        assert!(!cron_matches_tz("* * *", &now, "UTC"));
    }

    #[test]
    fn test_cron_matches_with_timezone() {
        // 12:00 UTC = 20:00 Asia/Singapore (UTC+8)
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        // Should match 20:00 in Singapore time
        assert!(cron_matches_tz("0 20 * * *", &now, "Asia/Singapore"));
        // Should NOT match 12:00 in Singapore time (that would be 04:00 UTC)
        assert!(!cron_matches_tz("0 12 * * *", &now, "Asia/Singapore"));
        // Should match 12:00 in UTC
        assert!(cron_matches_tz("0 12 * * *", &now, "UTC"));
    }
}
