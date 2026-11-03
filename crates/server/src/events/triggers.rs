//! The Composio trigger instances each user's topics need, made on their
//! connected accounts and disabled when a topic goes off. Their events come
//! back over the realtime channel (see `super::realtime`).

use super::{Topic, normalize};
use crate::{apps::connected_accounts, store::TriggerRow, user::User};
use anyhow::{Context, Result};
use serde_json::{Value, json};

/// A trigger a topic sets up on one toolkit's accounts.
struct TriggerSpec {
    topic: Topic,
    toolkit: &'static str,
    slug: &'static str,
    config: fn() -> Value,
}

fn primary_calendar() -> Value {
    json!({"calendarId": "primary"})
}

fn none() -> Value {
    json!({})
}

/// Each slug and config field was checked against Composio's v3.1 trigger
/// types. Polling triggers run on Composio's side; the server only listens.
const SPECS: &[TriggerSpec] = &[
    TriggerSpec {
        topic: Topic::Mail,
        toolkit: "gmail",
        slug: normalize::GMAIL_NEW_MESSAGE,
        config: || json!({"labelIds": "INBOX", "interval": 2}),
    },
    TriggerSpec {
        topic: Topic::Mail,
        toolkit: "outlook",
        slug: normalize::OUTLOOK_NEW_MESSAGE,
        config: none,
    },
    TriggerSpec {
        topic: Topic::CalendarChanges,
        toolkit: "googlecalendar",
        slug: normalize::GCAL_CREATED,
        config: primary_calendar,
    },
    TriggerSpec {
        topic: Topic::CalendarChanges,
        toolkit: "googlecalendar",
        slug: normalize::GCAL_UPDATED,
        config: primary_calendar,
    },
    TriggerSpec {
        topic: Topic::CalendarChanges,
        toolkit: "googlecalendar",
        slug: normalize::GCAL_CANCELLED,
        config: primary_calendar,
    },
    TriggerSpec {
        topic: Topic::CalendarChanges,
        toolkit: "outlook",
        slug: normalize::OUTLOOK_EVENT_CHANGE,
        config: none,
    },
    TriggerSpec {
        topic: Topic::CalendarUpcoming,
        toolkit: "googlecalendar",
        slug: normalize::GCAL_SOON,
        config: || json!({"calendarId": "primary", "countdownWindowMinutes": 30}),
    },
];

/// Makes the triggers `user`'s topics want on their connected accounts and
/// disables the ones they no longer want. Each trigger is tried on its own,
/// so one refusal leaves the others set up.
pub(crate) async fn sync_user(user: &User) -> Result<()> {
    let Some(client) = user.composio_client() else {
        return Ok(());
    };
    let accounts = connected_accounts(&client, user.external_id())
        .await
        .context("listing accounts")?;
    let topics = user.event_topics().await?;
    let existing = user.event_triggers().await?;

    for spec in SPECS {
        for account in accounts
            .iter()
            .filter(|account| account.toolkit_slug() == Some(spec.toolkit))
        {
            let have = existing
                .iter()
                .find(|row| row.slug == spec.slug && row.account_id == account.id);
            match (topics.is_on(spec.topic), have) {
                (true, None) => {
                    let made = client
                        .create_trigger(spec.slug, &account.id, (spec.config)())
                        .await;
                    let made = match made {
                        Ok(made) => made,
                        Err(error) => {
                            log::warn!(
                                "events: unable to make trigger {}: {error:#}; user={user}",
                                spec.slug
                            );
                            continue;
                        }
                    };
                    // An instance made before and disabled comes back disabled.
                    if let Err(error) = client.enable_trigger(&made.trigger_id).await {
                        log::warn!(
                            "events: unable to enable trigger {}: {error:#}; user={user}",
                            spec.slug
                        );
                    }
                    user.save_event_trigger(&TriggerRow {
                        slug: spec.slug.to_owned(),
                        account_id: account.id.clone(),
                        trigger_id: made.trigger_id,
                    })
                    .await?;
                    log::info!("events: trigger {} set up; user={user}", spec.slug);
                }
                (false, Some(row)) => match client.disable_trigger(&row.trigger_id).await {
                    Ok(()) => {
                        user.delete_event_trigger(row).await?;
                        log::info!("events: trigger {} disabled; user={user}", spec.slug);
                    }
                    Err(error) => log::warn!(
                        "events: unable to disable trigger {}: {error:#}; user={user}",
                        spec.slug
                    ),
                },
                _ => {}
            }
        }
    }

    // Composio drops the triggers of an account that is gone.
    for row in &existing {
        if !accounts.iter().any(|account| account.id == row.account_id) {
            user.delete_event_trigger(row).await?;
        }
    }
    Ok(())
}

/// Whether `user` has a trigger set up for new mail in `toolkit`.
pub(crate) async fn has_mail_trigger(user: &User, toolkit: &str) -> Result<bool> {
    let slugs: Vec<&str> = SPECS
        .iter()
        .filter(|spec| spec.topic == Topic::Mail && spec.toolkit == toolkit)
        .map(|spec| spec.slug)
        .collect();
    Ok(user
        .event_triggers()
        .await?
        .iter()
        .any(|row| slugs.contains(&row.slug.as_str())))
}
