//! Rate-limit-friendly Actions API calls: replies are cached by ETag (a 304 is free on GitHub),
//! and each token's reported rate limit pauses polling while low.

use crate::projects::git_items::{Caller, HostError, send_error, status_error};
use bytes::Bytes;
use reqwest::{StatusCode, Url, header, header::HeaderMap};
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    sync::{Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Replies kept for `If-None-Match`, server-wide.
const MAX_TAGGED: usize = 1024;
/// Below this many calls left, polling waits for the limit to reset.
pub(crate) const LOW_REMAINING: u64 = 200;
/// How long to wait after a rate-limited reply that says no reset time.
const LIMITED_WAIT: Duration = Duration::from_secs(60);

/// A reply kept with its ETag.
struct Tagged {
    etag: String,
    body: Bytes,
    /// When it was last used, by `HostCalls::uses`.
    used: u64,
}

/// What a host said about a token's rate limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limit {
    pub(crate) remaining: u64,
    pub(crate) reset: SystemTime,
}

/// Who a call is for; cache and rate limit are kept per user and account.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Who(pub(crate) String);

#[derive(Default)]
struct Kept {
    tagged: HashMap<String, Tagged>,
    limits: HashMap<Who, Limit>,
    uses: u64,
}

/// The kept replies and rate limits.
#[derive(Default)]
pub(crate) struct HostCalls {
    kept: Mutex<Kept>,
}

impl HostCalls {
    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Until when polling for `who` waits, when its rate limit runs low.
    pub(crate) fn limited_until(&self, who: &Who, now: SystemTime) -> Option<SystemTime> {
        let limit = *self.kept().limits.get(who)?;
        (limit.remaining < LOW_REMAINING && limit.reset > now).then_some(limit.reset)
    }

    fn note_limit(&self, who: &Who, headers: &HeaderMap, now: SystemTime) {
        if let Some(limit) = limit_of(headers, now) {
            self.kept().limits.insert(who.clone(), limit);
        }
    }

    /// `GET url` as JSON, from the cached copy when unchanged.
    pub(crate) async fn get_json<T: DeserializeOwned>(
        &self,
        caller: &Caller<'_>,
        who: &Who,
        url: &str,
    ) -> Result<T, HostError> {
        let body = self.get(caller, who, url).await?;
        serde_json::from_slice(&body).map_err(|e| {
            log::info!("reading {url} from {}: {e}", caller.host);
            HostError::BadReply(caller.host.to_owned())
        })
    }

    /// `GET url` with the token and cached ETag; the cached body after a 304.
    async fn get(&self, caller: &Caller<'_>, who: &Who, url: &str) -> Result<Bytes, HostError> {
        let host = caller.host.to_owned();
        let key = format!("{} {url}", who.0);
        let etag = self.kept().tagged.get(&key).map(|t| t.etag.clone());
        let _permit = caller
            .calls
            .acquire()
            .await
            .map_err(|_| HostError::Internal(host.clone()))?;
        let mut request = caller
            .client
            .get(url)
            .header(header::ACCEPT, "application/json")
            .header(
                header::AUTHORIZATION,
                format!("token {}", caller.token.expose_secret()),
            );
        if let Some(etag) = &etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        let reply = request.send().await.map_err(|e| send_error(&host, &e))?;
        let status = reply.status();
        let now = SystemTime::now();
        self.note_limit(who, reply.headers(), now);
        if status == StatusCode::NOT_MODIFIED {
            let mut kept = self.kept();
            kept.uses += 1;
            let uses = kept.uses;
            if let Some(tagged) = kept.tagged.get_mut(&key) {
                tagged.used = uses;
                return Ok(tagged.body.clone());
            }
            // Dropped since it was asked for: the host's 304 says nothing.
            return Err(HostError::BadReply(host));
        }
        if !status.is_success() {
            if matches!(
                status,
                StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
            ) {
                self.note_limited(who, reply.headers(), now);
            }
            return Err(status_error(host, status, reply.headers()));
        }
        let etag = reply
            .headers()
            .get(header::ETAG)
            .and_then(|tag| tag.to_str().ok())
            .map(str::to_owned);
        let body = reply.bytes().await.map_err(|e| send_error(&host, &e))?;
        if let Some(etag) = etag {
            self.keep(key, etag, body.clone());
        }
        Ok(body)
    }

    /// After a rate-limited refusal, no polling until the reset, or for a minute if unknown.
    fn note_limited(&self, who: &Who, headers: &HeaderMap, now: SystemTime) {
        let out = headers
            .get("x-ratelimit-remaining")
            .is_some_and(|left| left.as_bytes() == b"0");
        let retry = headers
            .get(header::RETRY_AFTER)
            .and_then(|after| after.to_str().ok()?.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        if !out && retry.is_none() {
            return;
        }
        let reset = limit_of(headers, now)
            .map(|limit| limit.reset)
            .or_else(|| retry.map(|wait| now + wait))
            .unwrap_or(now + LIMITED_WAIT);
        self.kept().limits.insert(
            who.clone(),
            Limit {
                remaining: 0,
                reset,
            },
        );
    }

    fn keep(&self, key: String, etag: String, body: Bytes) {
        let mut kept = self.kept();
        kept.uses += 1;
        let used = kept.uses;
        if kept.tagged.len() >= MAX_TAGGED && !kept.tagged.contains_key(&key) {
            let oldest = kept
                .tagged
                .iter()
                .min_by_key(|(_, tagged)| tagged.used)
                .map(|(key, _)| key.clone());
            if let Some(oldest) = oldest {
                kept.tagged.remove(&oldest);
            }
        }
        kept.tagged.insert(key, Tagged { etag, body, used });
    }

    /// The last `keep` bytes of `url`, reading at most `max`; a redirect is followed once without the token.
    pub(crate) async fn tail(
        &self,
        caller: &Caller<'_>,
        url: &str,
        limits: TailLimits,
    ) -> Result<Tail, HostError> {
        let host = caller.host.to_owned();
        let _permit = caller
            .calls
            .acquire()
            .await
            .map_err(|_| HostError::Internal(host.clone()))?;
        let first = caller
            .client
            .get(url)
            .header(
                header::AUTHORIZATION,
                format!("token {}", caller.token.expose_secret()),
            )
            .send()
            .await
            .map_err(|e| send_error(&host, &e))?;
        let mut reply = if first.status().is_redirection() {
            let to = first
                .headers()
                .get(header::LOCATION)
                .and_then(|to| to.to_str().ok())
                .and_then(|to| Url::parse(url).ok()?.join(to).ok())
                .filter(|to| matches!(to.scheme(), "https" | "http"))
                .ok_or_else(|| HostError::BadReply(host.clone()))?;
            caller
                .client
                .get(to)
                .send()
                .await
                .map_err(|e| send_error(&host, &e))?
        } else {
            first
        };
        let status = reply.status();
        if !status.is_success() {
            return Err(status_error(host, status, reply.headers()));
        }
        let mut kept: Vec<u8> = Vec::new();
        let mut read = 0usize;
        let mut cut = false;
        while let Some(chunk) = reply.chunk().await.map_err(|e| send_error(&host, &e))? {
            read += chunk.len();
            kept.extend_from_slice(&chunk);
            if kept.len() > limits.keep {
                let drop = kept.len() - limits.keep;
                kept.drain(..drop);
                cut = true;
            }
            if read >= limits.max {
                break;
            }
        }
        Ok(Tail {
            text: String::from_utf8_lossy(&kept).into_owned(),
            cut,
        })
    }
}

/// How much of a log `HostCalls::tail` keeps and reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TailLimits {
    pub(crate) keep: usize,
    pub(crate) max: usize,
}

/// The end of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tail {
    pub(crate) text: String,
    /// Bytes before `text` were left out.
    pub(crate) cut: bool,
}

/// The rate limit `headers` report, if they do.
fn limit_of(headers: &HeaderMap, now: SystemTime) -> Option<Limit> {
    let number =
        |name: &str| -> Option<u64> { headers.get(name)?.to_str().ok()?.trim().parse().ok() };
    let remaining = number("x-ratelimit-remaining")?;
    let reset = number("x-ratelimit-reset")
        .map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap_or(now + LIMITED_WAIT);
    Some(Limit { remaining, reset })
}

#[cfg(test)]
mod tests;
