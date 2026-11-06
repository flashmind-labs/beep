//! A window's request to a machine of the mesh, this Mac's own node among
//! them: it gets a time limit, and a failure or a request that runs out
//! leaves a line in the log naming the step, the machine and the session.

use super::Fleet;
use crate::error::{DesktopError, Result};
use iroh::EndpointId;
use std::{future::Future, time::Duration};
use tokio::time::timeout;

/// How long reading a session's log may take.
pub(crate) const READ_LIMIT: Duration = Duration::from_secs(20);
/// How long a prompt, a steer or a resume may take to be taken.
pub(crate) const ORDER_LIMIT: Duration = Duration::from_secs(30);
/// How long a small read about a session (usage, modes, tasks) may take.
pub(crate) const SMALL_LIMIT: Duration = Duration::from_secs(10);

impl Fleet {
    /// `call` to `machine` about `session`, given up after `limit`. `step`
    /// says what it does ("reading the session"), in the log and in the
    /// error the window shows.
    pub(crate) async fn asked<T>(
        &self,
        machine: EndpointId,
        session: &str,
        step: &str,
        limit: Duration,
        call: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        match timeout(limit, call).await {
            Ok(Ok(done)) => Ok(done),
            Ok(Err(e)) => {
                log::warn!(
                    "{step} failed; machine={} session={session} error={e}",
                    self.name_of(machine)
                );
                Err(e)
            }
            Err(_) => {
                let name = self.name_of(machine);
                let secs = limit.as_secs();
                log::warn!("{step}: no answer in {secs} s; machine={name} session={session}");
                Err(DesktopError::TimedOut(format!(
                    "{name} did not answer in {secs} s, {step}."
                )))
            }
        }
    }
}
