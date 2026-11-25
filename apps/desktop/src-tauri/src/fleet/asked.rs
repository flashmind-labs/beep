//! A window's request to a machine of the mesh, this Mac's own node among
//! them: it gets a time limit, and a failure or a request that runs out
//! leaves a line in the log naming the step, the machine and the session.

use super::Fleet;
use crate::error::{DesktopError, Result};
use beep_proto::SessionId;
use iroh::EndpointId;
use semver::Version;
use std::{future::Future, time::Duration};
use tokio::time::timeout;

/// How long reading a session's log may take.
pub(crate) const READ_LIMIT: Duration = Duration::from_secs(20);
/// How long a prompt, a steer or a resume may take to be taken.
pub(crate) const ORDER_LIMIT: Duration = Duration::from_secs(30);
/// How long a small read about a session (usage, modes, tasks) may take.
pub(crate) const SMALL_LIMIT: Duration = Duration::from_secs(10);

/// The last node that reads no `StopAll`.
pub(crate) const BEFORE_STOP_ALL: Version = Version::new(0, 5, 0);
/// The last node that reads no `SetDevice`.
pub(crate) const BEFORE_SET_DEVICE: Version = Version::new(0, 6, 28);
/// The last released node, which starts no home assistant. This computer's
/// own node is the desktop's build and is not held to it.
pub(crate) const BEFORE_ASSISTANT: Version = Version::new(0, 7, 12);
/// The last released node, which runs no named account. This computer's own
/// node is the desktop's build and is not held to it.
pub(crate) const BEFORE_ACCOUNTS: Version = Version::new(0, 8, 4);

/// The last released node, which moves no chat. This computer's own node is
/// the desktop's build and is not held to it.
pub(crate) const BEFORE_CHAT_MOVES: Version = Version::new(0, 8, 8);

/// Whether a node of `version` is newer than `last`, the last release
/// without a request. A node that has not said its version is taken as older.
pub(crate) fn newer_than(version: Option<&str>, last: &Version) -> bool {
    version
        .and_then(|version| Version::parse(version).ok())
        .is_some_and(|version| version > *last)
}

impl Fleet {
    /// `call` to `machine` about `session`, given up after `limit`. `step`
    /// says what it does ("reading the session"), in the log and in the
    /// error the window shows.
    pub(crate) async fn asked<T>(
        &self,
        machine: EndpointId,
        session: &SessionId,
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
