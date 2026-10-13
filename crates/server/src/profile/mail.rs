//! Mail, read a year at a time, newest first.

use super::{CONCURRENCY, Source, Summary, address, extract, top};
use crate::{
    decision::DecisionTool,
    feeds::mail::{
        Email, Mailbox, Source as MailSource, Window, clip, worth_learning, worth_reading,
    },
    starters::Generator,
    store::ProfileNote,
    user::User,
};
use anyhow::Result;
use chrono::{Datelike, Duration as Days, NaiveDate, Utc};
use futures::{StreamExt, TryStreamExt, future, stream};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Write,
};

/// Mail older than this is not looked for.
const OLDEST_YEAR: i32 = 1995;
/// Empty years in a row that end the mail read.
const EMPTY_YEARS: u32 = 3;
/// Most text from one year of mail the model reads.
const YEAR_CHARS: usize = 480_000;
/// Jev calls at once while screening a year of mail.
const SCREEN_CONCURRENCY: usize = 16;
/// Emails from one sender past which they are summed up, not read one by one.
const DIGEST_AFTER: usize = 10;
/// Emails of a repeated sender read in full.
const DIGEST_SAMPLES: usize = 5;
/// Subject lines a digest lists at most.
const DIGEST_SUBJECTS: usize = 80;
/// Body kept of an email the user sent, and of one anyone else did.
const SENT_CHARS: usize = 3000;
const RECEIVED_CHARS: usize = 1200;
const SAMPLE_CHARS: usize = 700;
/// Longest thread text.
const THREAD_CHARS: usize = 12_000;
/// Mail from these senders is skipped: dating apps and medical portals.
const SKIPPED_SENDERS: &[&str] = &[
    "tinder",
    "bumble",
    "hinge.co",
    "grindr",
    "okcupid",
    "match.com",
    "badoo",
    "happn",
    "feeld",
    "mychart",
    "patient",
    "clinic",
    "hospital",
    "pharmacy",
    "doctolib",
    "zocdoc",
    "healthcare",
];

const MAIL_SYSTEM: &str = "You help a personal assistant app get to know its user. You get part of one \
year of the user's own mailbox: threads they wrote in, summaries of senders who write often, and single \
emails. Mail marked as sent was written by the user.";

pub(super) async fn read_mail(
    generator: Generator<'_>,
    decision: Option<&DecisionTool>,
    user: &User,
    mailbox: Mailbox,
    summary: &mut Summary,
) -> Result<()> {
    let source = MailSource::new(user, mailbox).await?;
    let key = Source::Mail(mailbox).key();
    let mut before = user
        .profile_read(key)
        .await?
        .read_before
        .unwrap_or_else(Utc::now);
    let mut empty = 0;
    while empty < EMPTY_YEARS && before.year() > OLDEST_YEAR {
        let after = before - Days::days(365);
        let window = Window {
            after,
            before: Some(before),
        };
        let year = read_year(generator, decision, &source, window).await?;
        if year.emails == 0 {
            empty += 1;
        } else {
            empty = 0;
            summary.years += 1;
        }
        user.add_profile_notes(key, &year.notes, Some(after))
            .await?;
        summary.items += year.emails;
        summary.notes += year.notes.len();
        log::debug!(
            "profile: read mail before {before}; emails={} notes={}",
            year.emails,
            year.notes.len()
        );
        before = after;
    }
    Ok(())
}

/// What a year of mail gave.
struct Year {
    emails: usize,
    notes: Vec<ProfileNote>,
}

async fn read_year(
    generator: Generator<'_>,
    decision: Option<&DecisionTool>,
    source: &MailSource<'_>,
    window: Window,
) -> Result<Year> {
    let mut emails: Vec<Email> = Vec::new();
    let mut ids = HashSet::new();
    let mut pages = source.history(window).boxed();
    while let Some(page) = pages.try_next().await? {
        for mut email in page {
            let inside = email.received >= window.after
                && window.before.is_none_or(|before| email.received < before);
            if !inside || !ids.insert(email.id.clone()) {
                continue;
            }
            // Kept short while listing; a year can hold tens of thousands.
            let keep = if email.bulk { 0 } else { SENT_CHARS };
            email.body = clip(email.body.trim(), keep);
            emails.push(email);
        }
    }
    drop(pages);
    let count = emails.len();
    if count == 0 {
        return Ok(Year {
            emails: 0,
            notes: Vec::new(),
        });
    }
    let year = window.after.date_naive();
    let mut notes = vec![counts_note(year, &emails)];
    emails.retain(|email| !email.bulk && !skipped(&email.from));
    if let Some(decision) = decision {
        emails = screen(decision, emails).await;
    }
    let mut groups = group_mail(emails);
    fill_bodies(source, &mut groups).await;
    let texts: Vec<String> = groups.iter().map(Group::render).collect();
    let header = format!(
        "Mail from {} to {}.",
        window.after.format("%Y-%m-%d"),
        window.before.unwrap_or_else(Utc::now).format("%Y-%m-%d")
    );
    notes.extend(extract(generator, MAIL_SYSTEM, &header, &texts).await?);
    Ok(Year {
        emails: count,
        notes,
    })
}

/// Keeps mail the user wrote, the threads they wrote in, and received mail
/// Jev finds worth reading. Mailboxes without a bulk flag, like Fastmail,
/// rely on this to drop newsletters. Mail Jev fails on is kept.
async fn screen(decision: &DecisionTool, emails: Vec<Email>) -> Vec<Email> {
    let written: HashSet<String> = emails
        .iter()
        .filter(|email| email.sent)
        .filter_map(|email| email.thread.clone())
        .collect();
    let today = Utc::now().format("%A %Y-%m-%d").to_string();
    let count = emails.len();
    let kept: Vec<Email> = stream::iter(emails)
        .map(|email| {
            let today = &today;
            let mine = email.sent || email.thread.as_ref().is_some_and(|t| written.contains(t));
            async move {
                if mine {
                    return Some(email);
                }
                match worth_reading(decision, today, &email).await {
                    Ok(probability) if !worth_learning(probability) => None,
                    Ok(_) => Some(email),
                    Err(error) => {
                        log::debug!("profile: not screened: {error:#}");
                        Some(email)
                    }
                }
            }
        })
        .buffer_unordered(SCREEN_CONCURRENCY)
        .filter_map(future::ready)
        .collect()
        .await;
    log::debug!("profile: screened mail; kept={} of={count}", kept.len());
    kept
}

/// Mail the model reads together.
enum Group {
    /// A conversation the user wrote in, oldest first.
    Thread(Vec<Email>),
    /// A sender who wrote often: a few emails and the rest's subjects.
    Digest {
        sender: String,
        count: usize,
        samples: Vec<Email>,
        subjects: Vec<(NaiveDate, String)>,
    },
    Single(Email),
}

impl Group {
    /// Text the model reads, a rough size before bodies are filled.
    fn size(&self) -> usize {
        match self {
            Group::Thread(emails) => emails
                .iter()
                .map(|email| body_chars(email) + 200)
                .sum::<usize>()
                .min(THREAD_CHARS),
            Group::Digest {
                samples, subjects, ..
            } => samples.len() * (SAMPLE_CHARS + 200) + subjects.len() * 60,
            Group::Single(email) => body_chars(email) + 200,
        }
    }

    fn emails_mut(&mut self) -> Vec<&mut Email> {
        match self {
            Group::Thread(emails) => emails.iter_mut().collect(),
            Group::Digest { samples, .. } => samples.iter_mut().collect(),
            Group::Single(email) => vec![email],
        }
    }

    fn render(&self) -> String {
        let mut text = String::new();
        match self {
            Group::Thread(emails) => {
                let _ = writeln!(text, "Thread of {} emails:", emails.len());
                for email in emails {
                    let _ = writeln!(text, "---\n{}", email.render(body_chars(email)));
                }
                text = clip(&text, THREAD_CHARS);
            }
            Group::Digest {
                sender,
                count,
                samples,
                subjects,
            } => {
                let _ = writeln!(text, "{count} emails from {sender}. A few of them:");
                for email in samples {
                    let _ = writeln!(text, "---\n{}", email.render(SAMPLE_CHARS));
                }
                let _ = writeln!(text, "---\nAll their subjects:");
                for (day, subject) in subjects {
                    let _ = writeln!(text, "{day} {}", clip(subject, 120));
                }
            }
            Group::Single(email) => {
                let _ = writeln!(text, "Email:\n{}", email.render(body_chars(email)));
            }
        }
        text
    }
}

fn body_chars(email: &Email) -> usize {
    if email.sent {
        SENT_CHARS
    } else {
        RECEIVED_CHARS
    }
}

/// Threads the user wrote in first, then repeated senders, then the rest,
/// cut to what one year may send to the model.
fn group_mail(emails: Vec<Email>) -> Vec<Group> {
    let mut threads: HashMap<String, Vec<Email>> = HashMap::new();
    let mut loose = Vec::new();
    for email in emails {
        match email.thread.clone() {
            Some(thread) => threads.entry(thread).or_default().push(email),
            None => loose.push(email),
        }
    }
    let mut groups = Vec::new();
    let mut written: Vec<Vec<Email>> = Vec::new();
    for (_, mut thread) in threads {
        if thread.iter().any(|email| email.sent) {
            thread.sort_by_key(|email| email.received);
            written.push(thread);
        } else {
            loose.extend(thread);
        }
    }
    // Longest conversations first: those are the people who matter most.
    written.sort_by_key(|thread| Reverse(thread.len()));
    groups.extend(written.into_iter().map(Group::Thread));

    let mut senders: BTreeMap<String, Vec<Email>> = BTreeMap::new();
    for email in loose {
        senders.entry(address(&email.from)).or_default().push(email);
    }
    let mut digests = Vec::new();
    let mut singles = Vec::new();
    for (sender, mut emails) in senders {
        if emails.len() <= DIGEST_AFTER {
            singles.extend(emails.into_iter().map(Group::Single));
            continue;
        }
        emails.sort_by_key(|email| email.received);
        let count = emails.len();
        let subjects = emails
            .iter()
            .rev()
            .take(DIGEST_SUBJECTS)
            .rev()
            .map(|email| (email.received.date_naive(), email.subject.clone()))
            .collect();
        let step = count / DIGEST_SAMPLES;
        let samples = emails
            .into_iter()
            .step_by(step.max(1))
            .take(DIGEST_SAMPLES)
            .collect();
        digests.push(Group::Digest {
            sender,
            count,
            samples,
            subjects,
        });
    }
    // Mail the user sent first among the singles.
    singles.sort_by_key(|group| match group {
        Group::Single(email) => !email.sent,
        _ => true,
    });
    groups.extend(digests);
    groups.extend(singles);

    let mut total = 0;
    groups
        .into_iter()
        .take_while(|group| {
            total += group.size();
            total <= YEAR_CHARS
        })
        .collect()
}

/// Reads whole bodies where the listing only gave previews.
async fn fill_bodies(source: &MailSource<'_>, groups: &mut [Group]) {
    let emails: Vec<&mut Email> = groups.iter_mut().flat_map(Group::emails_mut).collect();
    stream::iter(emails)
        .for_each_concurrent(CONCURRENCY, |email| async move {
            source.fill_body(email).await;
            email.body = clip(email.body.trim(), SENT_CHARS);
        })
        .await;
}

/// A year's totals: who the user writes to, who writes to them, and what
/// they are subscribed to.
fn counts_note(year: NaiveDate, emails: &[Email]) -> ProfileNote {
    let mut to: HashMap<String, usize> = HashMap::new();
    let mut from: HashMap<String, usize> = HashMap::new();
    let mut bulk: HashMap<String, usize> = HashMap::new();
    let mut sent = 0;
    for email in emails {
        if email.sent {
            sent += 1;
            for recipient in email.to.split(',') {
                let recipient = recipient.trim();
                if !recipient.is_empty() {
                    *to.entry(recipient.to_owned()).or_default() += 1;
                }
            }
        } else if email.bulk {
            *bulk.entry(sender_name(&email.from)).or_default() += 1;
        } else {
            *from.entry(email.from.trim().to_owned()).or_default() += 1;
        }
    }
    let mut text = format!(
        "Mail in the year from {year}: {} emails, {sent} sent by the user.",
        emails.len()
    );
    for (label, counts) in [
        ("Writes most to", to),
        ("Gets most personal mail from", from),
        ("Gets most newsletters and promotions from", bulk),
    ] {
        if !counts.is_empty() {
            let _ = write!(text, " {label}: {}.", top(counts));
        }
    }
    ProfileNote { date: None, text }
}

/// The name in `Name <address>`, or the address's domain.
fn sender_name(from: &str) -> String {
    match from.trim().split_once('<') {
        Some((name, _)) if !name.trim().is_empty() => name.trim().trim_matches('"').to_owned(),
        _ => {
            let address = address(from);
            address.rsplit('@').next().unwrap_or(&address).to_owned()
        }
    }
}

fn skipped(from: &str) -> bool {
    let from = from.to_lowercase();
    SKIPPED_SENDERS.iter().any(|sender| from.contains(sender))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn email(id: &str, from: &str, sent: bool, thread: Option<&str>) -> Email {
        Email {
            id: id.into(),
            received: Utc::now(),
            from: from.into(),
            to: "Ana <ana@example.com>".into(),
            subject: format!("subject {id}"),
            sent,
            bulk: false,
            thread: thread.map(Into::into),
            body: "body".into(),
        }
    }

    #[test]
    fn threads_the_user_wrote_in_come_first_and_frequent_senders_are_summed_up() {
        let mut emails = vec![
            email("a", "Ana <ana@example.com>", false, Some("t1")),
            email("b", "me@example.com", true, Some("t1")),
            email("c", "Bo <bo@example.com>", false, Some("t2")),
        ];
        for n in 0..12 {
            emails.push(email(
                &format!("s{n}"),
                "Shop <news@shop.example>",
                false,
                None,
            ));
        }
        let groups = group_mail(emails);
        assert!(matches!(&groups[0], Group::Thread(thread) if thread.len() == 2));
        assert!(matches!(
            &groups[1],
            Group::Digest { sender, count: 12, samples, subjects }
                if sender == "news@shop.example" && samples.len() == DIGEST_SAMPLES && subjects.len() == 12
        ));
        assert!(matches!(&groups[2], Group::Single(email) if email.id == "c"));
    }

    #[test]
    fn senders_from_dating_and_medical_services_are_skipped() {
        assert!(skipped("Tinder <no-reply@gotinder.com>"));
        assert!(skipped("MyChart <noreply@mychart.org>"));
        assert!(!skipped("Zara <news@zara.com>"));
        assert_eq!(address("\"Ana\" <Ana@Example.com>"), "ana@example.com");
        assert_eq!(sender_name("Zara <news@zara.com>"), "Zara");
        assert_eq!(sender_name("news@zara.com"), "zara.com");
    }
}
