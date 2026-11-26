//! The cheap check of a GitHub project between lists (#403): its newest runs
//! asked with `If-None-Match` and the last ETag every `CHECK`. A 304 costs
//! no rate limit and means nothing changed; any other success means the runs
//! are listed in full. The host's own hints (`Retry-After`, an exhausted
//! rate limit, `X-Poll-Interval`) put the next check off, and failures back
//! off up to `QUIET`.

use anyhow::{Context, Result};
use beep_proto::SecretText;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, ETAG, HeaderMap, IF_NONE_MATCH},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

/// How often a GitHub project is checked.
pub(super) const CHECK: Duration = Duration::from_secs(45);
/// How often a quiet project's runs are listed, and the machines asked. A
/// check that keeps failing backs off up to this.
pub(super) const QUIET: Duration = Duration::from_secs(10 * 60);
/// The runs a check asks for: enough to see a new or changed one.
const RUNS: u32 = 5;
/// The most doublings of `CHECK` after failures in a row.
const MAX_DOUBLINGS: u32 = 4;

/// GitHub's answer to a check, as the watch reads it.
pub(super) struct Answer {
    pub(super) status: StatusCode,
    pub(super) headers: HeaderMap,
}

/// What the watch knows of one project's checks. In memory only.
#[derive(Debug, Default)]
pub(super) struct Checks {
    etag: Option<String>,
    next: Option<Instant>,
    failures: u32,
}

impl Checks {
    pub(super) fn due(&self, now: Instant) -> bool {
        self.next.is_none_or(|next| now >= next)
    }

    pub(super) fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    /// Takes the answer to a check made at `now`, the wall clock reading
    /// `wall`: true when the runs changed and are to be listed.
    pub(super) fn answered(
        &mut self,
        answer: Result<Answer>,
        now: Instant,
        wall: SystemTime,
    ) -> bool {
        let (changed, hint) = match answer {
            Ok(answer) => {
                let hint = hint(&answer.headers, wall);
                if answer.status == StatusCode::NOT_MODIFIED {
                    (Some(false), hint)
                } else if answer.status.is_success() {
                    if let Some(etag) = answer.headers.get(ETAG).and_then(|v| v.to_str().ok()) {
                        self.etag = Some(etag.to_owned());
                    }
                    (Some(true), hint)
                } else {
                    log::info!("CI watch: GitHub answered a check with {}", answer.status);
                    (None, hint)
                }
            }
            Err(e) => {
                log::debug!("CI watch: a check failed: {e:#}");
                (None, None)
            }
        };
        let wait = match changed {
            Some(_) => {
                self.failures = 0;
                CHECK
            }
            None => {
                self.failures = self.failures.saturating_add(1);
                let doublings = self.failures.min(MAX_DOUBLINGS);
                CHECK.saturating_mul(1 << doublings).min(QUIET)
            }
        };
        self.next = Some(now + wait.max(hint.unwrap_or_default()));
        changed.unwrap_or(false)
    }
}

/// How long the host asks to be left alone: the longest of `Retry-After`,
/// the time to `X-RateLimit-Reset` once `X-RateLimit-Remaining` is 0, and
/// `X-Poll-Interval`.
fn hint(headers: &HeaderMap, wall: SystemTime) -> Option<Duration> {
    let number =
        |name: &str| -> Option<u64> { headers.get(name)?.to_str().ok()?.trim().parse().ok() };
    let retry = number("retry-after").map(Duration::from_secs);
    let poll = number("x-poll-interval").map(Duration::from_secs);
    let reset = (number("x-ratelimit-remaining") == Some(0))
        .then(|| number("x-ratelimit-reset"))
        .flatten()
        .map(|reset| {
            let now = wall.duration_since(UNIX_EPOCH).unwrap_or_default();
            Duration::from_secs(reset).saturating_sub(now)
        });
    [retry, poll, reset].into_iter().flatten().max()
}

/// Asks GitHub for `repo`'s newest runs, unless they still match `etag`.
pub(super) async fn ask(
    client: &Client,
    token: &SecretText,
    repo: &str,
    etag: Option<&str>,
) -> Result<Answer> {
    let url = format!("https://api.github.com/repos/{repo}/actions/runs?per_page={RUNS}");
    let mut request = client
        .get(url)
        .bearer_auth(token.expose())
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }

    let response = request.send().await.context("asking GitHub")?;
    let status = response.status();
    let headers = response.headers().clone();
    // Read to the end, so the connection is kept for the next check.
    response.bytes().await.context("reading GitHub's answer")?;
    Ok(Answer { status, headers })
}
