//! The evening digest: the day's unread email, with what needs the user first.
//!
//! The daily mail sync flags emails that look like they need the user (see
//! `feeds::mail`). The digest is a routine of its own kind, made once when
//! the user's first mailbox syncs, that runs at 18:00 in the user's timezone.
//! It syncs the mailboxes first so the day's late mail is in, then lists the
//! mail still unread from the last day, plus older flagged mail still unread,
//! and wakes the agent in its chat with all of it, the flagged emails marked.
//! The agent picks what needs the user. It always replies, so a quiet day
//! reads as "nothing needs you" rather than as silence. The user turns it off
//! like any routine, on the Routines screen or by asking Beep.

use crate::{
    app::App,
    cron,
    feeds::{
        self,
        mail::{self, Mailbox},
    },
    protocol::Run,
    store::{AttentionItem, Feed},
    user::User,
};
use anyhow::Result;
use chrono::{DateTime, Duration as Days, Utc};
use flashmind::cron::{CronJob, JobSchedule};
use serde_json::json;
use std::{cmp::Reverse, fmt::Write, sync::Arc, time::Duration};
use uuid::Uuid;

/// The routine kind that runs [`run`].
pub const DIGEST_KIND: &str = "digest";
const SCHEDULE: &str = "0 18 * * *";
const LABEL: &str = "Evening digest";
const TASK: &str = "Write my evening digest of the unread emails below that need my attention. \
Most important first, one or two lines each: who it is from and what it asks of me. Keep it short.";
/// Read with the routine's task, which the store keeps as it was made.
const GUIDE: &str = "Flagged emails looked like they need me. Skip newsletters, marketing and \
automatic notifications unless they ask something of me. If nothing needs me, say that in one line. \
Your reply is always sent to me.";
/// Flagged emails older than this are dropped unreported.
const KEEP: Duration = Duration::from_secs(2 * 24 * 60 * 60);
/// How far back unflagged unread mail is read.
const WINDOW: Days = Days::days(1);
/// Unflagged emails past this many are left out, oldest first.
const MAX_EMAILS: usize = 40;
/// What the digest shows of an unflagged email.
const EMAIL_CHARS: usize = 800;

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

/// Whether `job` is a digest routine.
pub(crate) fn is_digest(job: &CronJob) -> bool {
    job.metadata.get("kind").and_then(|v| v.as_str()) == Some(DIGEST_KIND)
}

/// Makes the digest routine the first time it is called for `user`.
pub(crate) async fn offer(user: &User) {
    match user.offer_digest(&routine(user.id())).await {
        Ok(true) => log::info!("digest: routine made; user={user}"),
        Ok(false) => {}
        Err(error) => log::warn!("digest: unable to make the routine: {error:#}; user={user}"),
    }
}

/// Starts the digest run for `job`, or `None` when no mailbox is connected.
pub(crate) async fn run(app: &Arc<App>, user: &Arc<User>, job: &CronJob) -> Result<Option<Run>> {
    let mailboxes: Vec<Mailbox> = user
        .feeds()
        .await
        .into_iter()
        .filter_map(|feed| match feed {
            Feed::Mail(mailbox) => Some(mailbox),
            _ => None,
        })
        .collect();
    if mailboxes.is_empty() {
        log::info!("digest: no mailbox connected; user={user}");
        return Ok(None);
    }
    let feeds: Vec<Feed> = mailboxes.iter().copied().map(Feed::Mail).collect();
    feeds::sync_now(app, user, &feeds).await;

    let flagged = user.attention_items(KEEP).await?;
    let since = Utc::now() - WINDOW;
    let mut report = Report::default();
    let mut handled = Vec::new();
    for mailbox in mailboxes {
        let mine: Vec<&AttentionItem> = flagged
            .iter()
            .filter(|item| item.mailbox == mailbox)
            .collect();
        let after = mine
            .iter()
            .map(|item| item.received)
            .min()
            .map_or(since, |oldest| oldest.min(since));
        let unread = match mail::unread_after(user, mailbox, after).await {
            Ok(unread) => unread,
            Err(error) => {
                log::warn!(
                    "digest: unable to list unread {}: {error:#}; user={user}",
                    mailbox.slug()
                );
                report.failed.push(mailbox);
                continue;
            }
        };
        report.checked.push(mailbox);
        for email in unread {
            let item = mine.iter().find(|item| item.message_id == email.id);
            if item.is_none() && (email.received < since || email.bulk) {
                continue;
            }
            report.emails.push(DigestEmail {
                mailbox,
                received: email.received,
                text: item.map_or_else(|| email.render(EMAIL_CHARS), |item| item.text.clone()),
                flagged: item.is_some(),
            });
        }
        let ids: Vec<String> = mine.iter().map(|item| item.message_id.clone()).collect();
        handled.push((mailbox, ids));
    }
    report.trim();
    log::info!(
        "digest: {} unread, {} flagged; user={user}",
        report.emails.len(),
        report.emails.iter().filter(|email| email.flagged).count()
    );

    let run = cron::submit(app, user, job, &prompt(job, &report)).await?;
    // The run has started, so a failure here must not report the routine as
    // failed; the emails stay flagged for the next digest.
    for (mailbox, ids) in handled {
        if let Err(error) = user.clear_attention(mailbox, &ids).await {
            log::warn!(
                "digest: unable to clear {}: {error:#}; user={user}",
                mailbox.slug()
            );
        }
    }
    Ok(run)
}

/// What the digest found across the mailboxes.
#[derive(Debug, Default)]
struct Report {
    checked: Vec<Mailbox>,
    /// Mailboxes whose unread mail could not be listed.
    failed: Vec<Mailbox>,
    emails: Vec<DigestEmail>,
}

impl Report {
    /// Newest first, keeping every flagged email and at most `MAX_EMAILS`
    /// in all otherwise.
    fn trim(&mut self) {
        self.emails
            .sort_by(|a, b| b.flagged.cmp(&a.flagged).then(b.received.cmp(&a.received)));
        let keep = self
            .emails
            .iter()
            .filter(|email| email.flagged)
            .count()
            .max(MAX_EMAILS);
        self.emails.truncate(keep);
        self.emails.sort_by_key(|email| Reverse(email.received));
    }
}

/// One unread email as the digest shows it.
#[derive(Debug)]
struct DigestEmail {
    mailbox: Mailbox,
    received: DateTime<Utc>,
    text: String,
    /// The sync judged it to need the user.
    flagged: bool,
}

fn names(mailboxes: &[Mailbox]) -> String {
    let names: Vec<_> = mailboxes.iter().map(|mailbox| mailbox.name()).collect();
    names.join(", ")
}

/// The routine's task, how to stop it and the emails.
fn prompt(job: &CronJob, report: &Report) -> String {
    let mut text = format!(
        "{}\n\n{GUIDE}\n\nIf I ask to stop these, turn off routine {} with cron_edit.",
        job.task, job.id
    );
    if !report.checked.is_empty() {
        let _ = write!(text, "\n\nChecked: {}.", names(&report.checked));
    }
    if !report.failed.is_empty() {
        let _ = write!(
            text,
            "\nCould not check: {}. Say so.",
            names(&report.failed)
        );
    }
    if report.emails.is_empty() {
        text.push_str("\n\nUnread emails from the last day: none.");
        return text;
    }
    let _ = write!(
        text,
        "\n\nUnread emails from the last day ({}):",
        report.emails.len()
    );
    for email in &report.emails {
        let mark = if email.flagged { ", flagged" } else { "" };
        let _ = write!(
            text,
            "\n\n--- {}{mark}\n{}",
            email.mailbox.name(),
            email.text
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(mailbox: Mailbox, hours_ago: i64, text: &str, flagged: bool) -> DigestEmail {
        DigestEmail {
            mailbox,
            received: Utc::now() - Days::hours(hours_ago),
            text: text.to_owned(),
            flagged,
        }
    }

    #[test]
    fn prompt_names_the_routine_each_mailbox_and_the_flagged_emails() {
        let job = routine("user");
        let report = Report {
            checked: vec![Mailbox::Gmail, Mailbox::Outlook],
            failed: vec![Mailbox::Fastmail],
            emails: vec![
                email(Mailbox::Gmail, 1, "Subject: Lease", true),
                email(Mailbox::Outlook, 2, "Subject: Invoice", false),
            ],
        };
        let text = prompt(&job, &report);
        assert!(text.starts_with(TASK));
        assert!(text.contains(&format!("turn off routine {}", job.id)));
        assert!(text.contains("Checked: Gmail, Outlook."));
        assert!(text.contains("Could not check: Fastmail."));
        assert!(text.contains("Unread emails from the last day (2):"));
        assert!(text.contains("--- Gmail, flagged\nSubject: Lease"));
        assert!(text.contains("--- Outlook\nSubject: Invoice"));
    }

    #[test]
    fn the_digest_routine_is_always_sent() {
        assert!(is_digest(&routine("alice")));
        let reminder = CronJob {
            metadata: json!({"user_id": "alice", "label": "Water"}),
            ..routine("alice")
        };
        assert!(!is_digest(&reminder));
    }

    #[test]
    fn an_empty_day_still_gets_a_prompt() {
        let report = Report {
            checked: vec![Mailbox::Gmail],
            ..Report::default()
        };
        let text = prompt(&routine("user"), &report);
        assert!(text.contains("If nothing needs me, say that in one line."));
        assert!(text.contains("Your reply is always sent to me."));
        assert!(text.ends_with("Unread emails from the last day: none."));
    }

    #[test]
    fn trimming_keeps_flagged_emails_and_the_newest_others() {
        let mut report = Report::default();
        report
            .emails
            .push(email(Mailbox::Gmail, 30, "old flagged", true));
        for hours in 0..MAX_EMAILS as i64 + 5 {
            report
                .emails
                .push(email(Mailbox::Gmail, hours, &format!("{hours}"), false));
        }
        report.trim();
        assert_eq!(report.emails.len(), MAX_EMAILS);
        assert!(
            report
                .emails
                .iter()
                .any(|email| email.text == "old flagged")
        );
        assert_eq!(report.emails[0].text, "0");
        assert!(!report.emails.iter().any(|email| email.text == "39"));
    }
}
