//! The evening digest: recent email that needs the user and is still unread.
//!
//! The daily mail sync flags emails for it (see `feeds::mail`). The digest is
//! a routine of its own kind, made once when the user's first mailbox syncs,
//! that runs at 18:00 in the user's timezone. It syncs the mailboxes first so
//! the day's late mail is in, drops the flagged emails the user has read
//! since, and wakes the agent in a new chat with the rest. With nothing left
//! it sends nothing. The user turns it off like any routine, on the Routines
//! screen or by asking Beep.

use crate::{
    app::App,
    cron,
    feeds::{self, mail},
    protocol::Run,
    store::{AttentionItem, Feed},
    user::User,
};
use anyhow::Result;
use chrono::Utc;
use flashmind::cron::{CronJob, JobSchedule};
use serde_json::json;
use std::{fmt::Write, sync::Arc, time::Duration};
use uuid::Uuid;

/// The routine kind that runs [`run`].
pub const DIGEST_KIND: &str = "digest";
const SCHEDULE: &str = "0 18 * * *";
const LABEL: &str = "Evening digest";
const TASK: &str = "Write my evening digest of the unread emails below that need my attention. \
Most important first, one or two lines each: who it is from and what it asks of me. Keep it short.";
/// Flagged emails older than this are dropped unreported.
const KEEP: Duration = Duration::from_secs(2 * 24 * 60 * 60);

/// The digest routine for `user_id`.
fn routine(user_id: &str) -> CronJob {
    CronJob {
        id: Uuid::new_v4(),
        schedule: JobSchedule::Cron(SCHEDULE.to_owned()),
        task: TASK.to_owned(),
        enabled: true,
        once: false,
        metadata: json!({"user_id": user_id, "label": LABEL, "kind": DIGEST_KIND}),
        created_at: Utc::now(),
        last_run: None,
    }
}

/// Makes the digest routine the first time it is called for `user`.
pub(crate) async fn offer(user: &User) {
    match user.offer_digest(&routine(user.id())).await {
        Ok(true) => log::info!("digest: routine made; user={user}"),
        Ok(false) => {}
        Err(error) => log::warn!("digest: unable to make the routine: {error:#}; user={user}"),
    }
}

/// Starts the digest run for `job`, or `None` when nothing is unread.
pub(crate) async fn run(app: &Arc<App>, user: &Arc<User>, job: &CronJob) -> Result<Option<Run>> {
    let mailboxes: Vec<_> = user
        .feeds()
        .await
        .into_iter()
        .filter_map(|feed| match feed {
            Feed::Mail(mailbox) => Some(mailbox),
            Feed::Slack => None,
        })
        .collect();
    let feeds: Vec<_> = mailboxes
        .iter()
        .map(|&mailbox| Feed::Mail(mailbox))
        .collect();
    feeds::sync_now(app, user, &feeds).await;

    let items = user.attention_items(KEEP).await?;
    let mut unread: Vec<AttentionItem> = Vec::new();
    let mut handled = Vec::new();
    for mailbox in mailboxes {
        let flagged: Vec<_> = items
            .iter()
            .filter(|item| item.mailbox == mailbox)
            .collect();
        let Some(oldest) = flagged.iter().map(|item| item.received).min() else {
            continue;
        };
        let ids = match mail::unread_since(user, mailbox, oldest).await {
            Ok(ids) => ids,
            Err(error) => {
                log::warn!(
                    "digest: unable to list unread {}: {error:#}; user={user}",
                    mailbox.slug()
                );
                continue;
            }
        };
        unread.extend(
            flagged
                .iter()
                .filter(|item| ids.contains(&item.message_id))
                .map(|&item| item.clone()),
        );
        let ids: Vec<String> = flagged.iter().map(|item| item.message_id.clone()).collect();
        handled.push((mailbox, ids));
    }
    log::info!(
        "digest: {} unread of {} flagged; user={user}",
        unread.len(),
        items.len()
    );

    let run = if unread.is_empty() {
        None
    } else {
        unread.sort_by_key(|item| item.received);
        cron::submit(app, user, job, &prompt(job, &unread)).await?
    };
    for (mailbox, ids) in handled {
        user.clear_attention(mailbox, &ids).await?;
    }
    Ok(run)
}

/// The routine's task, how to stop it and the emails.
fn prompt(job: &CronJob, unread: &[AttentionItem]) -> String {
    let mut text = format!(
        "{}\n\nIf I ask to stop these, turn off routine {} with cron_edit.\n\nUnread emails ({}):",
        job.task,
        job.id,
        unread.len()
    );
    for item in unread {
        let _ = write!(text, "\n\n--- {}\n{}", item.mailbox.name(), item.text);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feeds::mail::Mailbox;

    #[test]
    fn prompt_names_the_routine_and_each_mailbox() {
        let job = routine("user");
        let item = |mailbox, text: &str| AttentionItem {
            mailbox,
            message_id: "m".to_owned(),
            received: Utc::now(),
            text: text.to_owned(),
        };
        let text = prompt(
            &job,
            &[
                item(Mailbox::Gmail, "Subject: Lease"),
                item(Mailbox::Outlook, "Subject: Invoice"),
            ],
        );
        assert!(text.starts_with(TASK));
        assert!(text.contains(&format!("turn off routine {}", job.id)));
        assert!(text.contains("Unread emails (2):"));
        assert!(text.contains("--- Gmail\nSubject: Lease"));
        assert!(text.contains("--- Outlook\nSubject: Invoice"));
    }
}
