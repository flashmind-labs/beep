//! The profile pass: reads each connected mailbox and calendar once and builds a profile of the user.
//! Notes are stored per finished year, so a cut-short pass resumes; content is never logged.

mod condense;
mod notes;
mod source;
use condense::condense;
use notes::{CONCURRENCY, DOSSIER_SYSTEM, extract, pack, parse_day};
use source::sources;
pub use source::{Calendar, Source};

use crate::{
    account::user::User,
    app::{App, Reach, UserNotice},
    protocol::{MemoryRead, Notice},
    store::MemoryRow,
};
use anyhow::Result;
use chrono::{DateTime, SecondsFormat, Utc};
use std::{
    collections::{HashMap, HashSet},
    fmt, mem,
    sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError},
};

mod calendar;
mod mail;

use calendar::read_calendar;
use mail::read_mail;

/// People and senders a yearly count note names.
const TOP_COUNT: usize = 15;

/// Sources being read, by user, so a second request leaves them alone.
static RUNNING: LazyLock<Mutex<HashSet<(String, Source)>>> = LazyLock::new(Mutex::default);

/// A source being read; dropping it frees the source.
struct Claim {
    user: String,
    source: Source,
}

impl Claim {
    fn take(user: &User, source: Source) -> Option<Self> {
        Self::take_id(user.id().to_owned(), source)
    }

    fn take_id(user: String, source: Source) -> Option<Self> {
        let inserted = running().insert((user.clone(), source));
        inserted.then_some(Claim { user, source })
    }

    /// Frees the source and says, under one lock, whether no other source of the user is still read.
    fn release_last(mut self) -> bool {
        let user = mem::take(&mut self.user);
        let mut running = running();
        running.remove(&(user.clone(), self.source));
        let last = !running.iter().any(|(id, _)| *id == user);
        drop(running);
        // Already freed above.
        mem::forget(self);
        last
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        running().remove(&(mem::take(&mut self.user), self.source));
    }
}

fn running() -> MutexGuard<'static, HashSet<(String, Source)>> {
    RUNNING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads, in the background, each unread source behind the connected apps `slugs`.
pub fn spawn(app: &Arc<App>, user: &Arc<User>, slugs: &[String]) {
    for source in sources(slugs) {
        let Some(claim) = Claim::take(user, source) else {
            continue;
        };
        tokio::spawn(read(app.clone(), user.clone(), source, claim));
    }
}

/// Rereads each source behind `slugs` from the start, dropping its notes; returns the sources started.
pub async fn reread(app: &Arc<App>, user: &Arc<User>, slugs: &[String]) -> Result<Vec<Source>> {
    let mut started = Vec::new();
    for source in sources(slugs) {
        let Some(claim) = Claim::take(user, source) else {
            continue;
        };
        user.reset_profile_read(source.key()).await?;
        tokio::spawn(read(app.clone(), user.clone(), source, claim));
        started.push(source);
    }
    Ok(started)
}

async fn read(app: Arc<App>, user: Arc<User>, source: Source, claim: Claim) {
    match user.profile_read(source.key()).await {
        Ok(read) if read.finished => return,
        Ok(_) => {}
        Err(error) => {
            log::warn!("profile: unable to read state: {error:#}; user={user}");
            return;
        }
    }
    log::info!("profile: reading {source}; user={user}");
    let generator = app.generator(user.id());
    let mut summary = Summary::default();
    let read = match source {
        Source::Mail(mailbox) => {
            read_mail(generator, app.decision(), &user, mailbox, &mut summary).await
        }
        Source::Calendar(calendar) => read_calendar(generator, &user, calendar, &mut summary).await,
    };
    if let Err(error) = read {
        // Not finished, so the next listing of apps resumes it. What it noted
        // so far is condensed now, or it would wait for a read that may keep
        // stopping.
        log::warn!("profile: read of {source} stopped: {error:#}; user={user} {summary}");
        if claim.release_last() {
            match condense(generator, &user, source).await {
                Ok(learned) => log::info!("profile: condensed; user={user} learned={learned}"),
                Err(error) => log::warn!("profile: condensing stopped: {error:#}; user={user}"),
            }
        }
        return;
    }
    if let Err(error) = user.finish_profile_read(source.key()).await {
        log::warn!("profile: unable to save state: {error:#}; user={user}");
        return;
    }
    log::info!("profile: read {source}; user={user} {summary}");
    if !claim.release_last() {
        return;
    }
    match condense(generator, &user, source).await {
        Ok(learned) => notify(&user, source, &summary, learned),
        Err(error) => log::warn!("profile: condensing stopped: {error:#}; user={user}"),
    }
}

/// What one source gave.
#[derive(Debug, Default)]
struct Summary {
    years: u32,
    items: usize,
    notes: usize,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "years={} items={} notes={}",
            self.years, self.items, self.notes
        )
    }
}

fn notify(user: &User, source: Source, summary: &Summary, learned: usize) {
    let read = match source {
        Source::Mail(_) if summary.years == 1 => "a year of your mail".to_owned(),
        Source::Mail(_) => format!("{} years of your mail", summary.years),
        Source::Calendar(_) => "your calendar".to_owned(),
    };
    let notice = UserNotice {
        user: user.id().to_owned(),
        notice: Notice {
            title: "Beep got to know you".to_owned(),
            body: format!("Read {read} and learned {learned} things. See Memories."),
            session_id: String::new(),
            app: None,
            approval: None,
            changed: false,
            group: false,
        },
        desktop: None,
        to: Reach::Everyone,
    };
    // Nobody listening means the phone is offline; Memories still shows them.
    let _ = user.notifications.send(notice);
}

fn top(counts: HashMap<String, usize>) -> String {
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts
        .into_iter()
        .take(TOP_COUNT)
        .map(|(name, count)| format!("{name} ({count})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The address in `Name <address>`, lowercased.
fn address(from: &str) -> String {
    let from = from.trim();
    let inner = match (from.rfind('<'), from.rfind('>')) {
        (Some(start), Some(end)) if start < end => &from[start + 1..end],
        _ => from,
    };
    inner.trim().to_lowercase()
}

fn rfc3339(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Marks a memory a read added or confirmed: `read:<source key>:<finished>`.
const READ_PREFIX: &str = "read:";

fn read_reference(source: Source, finished: DateTime<Utc>) -> String {
    format!("{READ_PREFIX}{}:{}", source.key(), rfc3339(finished))
}

/// The reads that added or confirmed `rows`, newest first.
pub fn memory_reads(rows: &[MemoryRow]) -> Vec<MemoryRead> {
    let mut reads: Vec<MemoryRead> = Vec::new();
    for row in rows {
        let Some(reference) = row.origin.as_ref().map(|origin| &origin.reference) else {
            continue;
        };
        let Some((key, finished_at)) = reference
            .strip_prefix(READ_PREFIX)
            .and_then(|rest| rest.split_once(':'))
        else {
            continue;
        };
        let Some(source) = Source::ALL.into_iter().find(|source| source.key() == key) else {
            continue;
        };
        match reads.iter_mut().find(|read| &read.id == reference) {
            Some(read) => read.memories.push(row.id.clone()),
            None => reads.push(MemoryRead {
                id: reference.clone(),
                source: source.name().to_owned(),
                finished_at: finished_at.to_owned(),
                memories: vec![row.id.clone()],
            }),
        }
    }
    // RFC 3339 in UTC to the second sorts as text.
    reads.sort_by(|a, b| b.finished_at.cmp(&a.finished_at));
    reads
}

#[cfg(test)]
mod tests {
    use super::notes::parse_notes;
    use super::*;
    use crate::apps::feeds::mail::Mailbox;

    #[test]
    fn only_the_last_read_to_end_condenses() {
        let claim = |source| Claim::take_id("release-last-test".into(), source).unwrap();
        let gmail = claim(Source::Mail(Mailbox::Gmail));
        let calendar = claim(Source::Calendar(Calendar::Google));
        assert!(!gmail.release_last());
        assert!(calendar.release_last());
        assert!(!running().iter().any(|(id, _)| id == "release-last-test"));
    }

    #[test]
    fn a_cut_off_answer_keeps_its_whole_notes() {
        let notes = parse_notes(
            "{\"notes\": [{\"note\": \"Flew to Tokyo.\"}, {\"note\": \"Speaks Spanish.\"}, {\"date\": \"2024-",
        )
        .unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].text, "Speaks Spanish.");
        assert!(parse_notes("{\"notes\": [{\"note\": \"Fle").is_err());
    }
}
