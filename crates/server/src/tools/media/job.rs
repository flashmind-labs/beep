//! Waiting on a service's job: bounded in time, stopped with the turn.

use flashmind::types::{CompletionStream, StreamEvent};
use futures::StreamExt;
use std::{future::Future, time::Duration};
use thiserror::Error;
use tokio::time::{Instant, sleep_until};
use tokio_util::sync::CancellationToken;

/// A file a service made.
#[derive(Debug)]
pub(super) struct Produced {
    pub(super) media_type: String,
    pub(super) data: Vec<u8>,
}

#[derive(Debug, Error)]
pub(super) enum JobError {
    #[error("it did not finish within {} minutes", .0.as_secs() / 60)]
    TimedOut(Duration),
    #[error("it was stopped")]
    Cancelled,
    #[error("{0:#}")]
    Failed(anyhow::Error),
}

/// The first file `stream` yields, giving up after `limit` or on `cancel`.
pub(super) async fn first_file(
    mut stream: CompletionStream,
    limit: Duration,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(&str) + Send),
) -> Result<Produced, JobError> {
    let deadline = Instant::now() + limit;
    loop {
        let event = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(JobError::Cancelled),
            () = sleep_until(deadline) => return Err(JobError::TimedOut(limit)),
            event = stream.next() => event,
        };
        match event {
            None | Some(Ok(StreamEvent::Finished(_))) => {
                return Err(JobError::Failed(anyhow::anyhow!(
                    "the service finished without sending a file"
                )));
            }
            Some(Err(error)) => return Err(JobError::Failed(error)),
            Some(Ok(StreamEvent::FileAttachment {
                media_type, data, ..
            })) => return Ok(Produced { media_type, data }),
            Some(Ok(StreamEvent::ContentDelta(text))) => {
                for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                    progress(line);
                }
            }
            Some(Ok(_)) => {}
        }
    }
}

/// `job`, given up after `limit` or once `cancel` fires.
pub(super) async fn bounded<T>(
    job: impl Future<Output = anyhow::Result<T>>,
    limit: Duration,
    cancel: &CancellationToken,
) -> Result<T, JobError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(JobError::Cancelled),
        () = tokio::time::sleep(limit) => Err(JobError::TimedOut(limit)),
        done = job => done.map_err(JobError::Failed),
    }
}
