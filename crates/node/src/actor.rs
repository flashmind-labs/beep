//! Timing for the node's actors. `channel` makes an actor's mailbox: each
//! message records how long it waited (from the start of its send, so
//! waiting for room counts) and how long the actor handled it, and `job`
//! times work an actor runs off its loop. Labels are fixed names: the
//! actor's, the message enum's variant (`strum::IntoStaticStr`) and the job's.
//!
//! Metrics: `beep_actor_wait_seconds{actor,message}`,
//! `beep_actor_handle_seconds{actor,message}`,
//! `beep_actor_job_seconds{actor,job}`. An `INFO` line only for a message
//! that waited `SLOW_WAIT`, took `SLOW_HANDLE`, or a job that took `SLOW_JOB`.

use std::time::Duration;
use tokio::{
    sync::mpsc::{self, error::SendError, error::TrySendError},
    time::Instant,
};

/// A message that waited this long is logged.
pub const SLOW_WAIT: Duration = Duration::from_millis(100);
/// A message that took this long to handle is logged.
pub const SLOW_HANDLE: Duration = Duration::from_millis(50);
/// A job that took this long is logged.
pub const SLOW_JOB: Duration = Duration::from_secs(1);

/// A message and when its sender started to send it.
struct Stamped<M> {
    message: M,
    sent: Instant,
}

/// The sending half of an actor's mailbox.
pub struct Sender<M> {
    actor: &'static str,
    mailbox: mpsc::Sender<Stamped<M>>,
}

impl<M> Clone for Sender<M> {
    fn clone(&self) -> Self {
        Self {
            actor: self.actor,
            mailbox: self.mailbox.clone(),
        }
    }
}

/// A place in the mailbox, taken by `Sender::reserve_owned`.
pub struct Place<M>(mpsc::OwnedPermit<Stamped<M>>);

/// The receiving half of an actor's mailbox.
pub struct Receiver<M> {
    actor: &'static str,
    mailbox: mpsc::Receiver<Stamped<M>>,
}

/// A mailbox for `actor` that holds `capacity` messages.
pub fn channel<M>(actor: &'static str, capacity: usize) -> (Sender<M>, Receiver<M>)
where
    for<'a> &'a M: Into<&'static str>,
{
    let (mailbox_tx, mailbox) = mpsc::channel(capacity);

    (
        Sender {
            actor,
            mailbox: mailbox_tx,
        },
        Receiver { actor, mailbox },
    )
}

impl<M> Sender<M> {
    pub async fn send(&self, message: M) -> Result<(), SendError<M>> {
        let sent = Instant::now();

        self.mailbox
            .send(Stamped { message, sent })
            .await
            .map_err(|SendError(stamped)| SendError(stamped.message))
    }

    pub fn try_send(&self, message: M) -> Result<(), TrySendError<M>> {
        let sent = Instant::now();

        self.mailbox
            .try_send(Stamped { message, sent })
            .map_err(|e| match e {
                TrySendError::Full(stamped) => TrySendError::Full(stamped.message),
                TrySendError::Closed(stamped) => TrySendError::Closed(stamped.message),
            })
    }

    /// Waits for room and holds it. `Place::send` then cannot wait or fail.
    pub async fn reserve_owned(self) -> Result<Place<M>, SendError<()>> {
        self.mailbox.reserve_owned().await.map(Place)
    }

    pub fn is_closed(&self) -> bool {
        self.mailbox.is_closed()
    }

    /// How many senders there are, this one included.
    pub fn strong_count(&self) -> usize {
        self.mailbox.strong_count()
    }
}

impl<M> Place<M> {
    pub fn send(self, message: M) {
        self.0.send(Stamped {
            message,
            sent: Instant::now(),
        });
    }
}

impl<M> Receiver<M>
where
    for<'a> &'a M: Into<&'static str>,
{
    /// The next message, with the timing of its handling: hold the guard
    /// until the message is dealt with.
    pub async fn recv(&mut self) -> Option<(M, Handling)> {
        let Stamped { message, sent } = self.mailbox.recv().await?;
        let waited = sent.elapsed();
        let label = Into::<&'static str>::into(&message);
        metrics::histogram!("beep_actor_wait_seconds", "actor" => self.actor, "message" => label)
            .record(waited.as_secs_f64());

        Some((
            message,
            Handling {
                actor: self.actor,
                message: label,
                waited,
                taken: Instant::now(),
            },
        ))
    }
}

impl<M> Receiver<M> {
    /// Refuses new messages; the ones queued can still be received.
    pub fn close(&mut self) {
        self.mailbox.close();
    }
}

/// Records how long a message took to handle when dropped.
#[must_use = "dropping it at once records a handling time of zero"]
pub struct Handling {
    actor: &'static str,
    message: &'static str,
    waited: Duration,
    taken: Instant,
}

impl Drop for Handling {
    fn drop(&mut self) {
        let handled = self.taken.elapsed();
        metrics::histogram!("beep_actor_handle_seconds", "actor" => self.actor, "message" => self.message)
            .record(handled.as_secs_f64());

        if self.waited >= SLOW_WAIT || handled >= SLOW_HANDLE {
            tracing::info!(
                actor = self.actor,
                message = self.message,
                waited_ms = self.waited.as_millis(),
                handled_ms = handled.as_millis(),
                "slow actor message"
            );
        }
    }
}

/// Runs `work`, which `actor` does off its loop, and records how long it took.
pub async fn job<T>(actor: &'static str, job: &'static str, work: impl Future<Output = T>) -> T {
    let started = Instant::now();
    let done = work.await;
    let took = started.elapsed();
    metrics::histogram!("beep_actor_job_seconds", "actor" => actor, "job" => job)
        .record(took.as_secs_f64());

    if took >= SLOW_JOB {
        tracing::info!(actor, job, took_ms = took.as_millis(), "slow actor job");
    }

    done
}

#[cfg(test)]
#[path = "tests/actor.rs"]
mod tests;
