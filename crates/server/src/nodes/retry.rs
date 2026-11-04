//! Calls to a node that go again on its next connection when its link drops
//! under them, as it does when the machine changes network or `beep-node`
//! restarts.

use super::Online;
use std::time::Duration;
use tokio::time::Instant;

/// How long a call waits for a node whose link dropped to connect again.
pub(crate) const NODE_BACK: Duration = Duration::from_secs(60);

/// A failure before the node got the request: the stream did not open or
/// the request could not be written. Such a call may always go again.
#[derive(Debug, thiserror::Error)]
#[error("{0:#}")]
pub(crate) struct Unsent(pub(crate) anyhow::Error);

/// What a call may do again when the link drops under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Repeat {
    /// It reads, or doing it twice does no harm: it goes again.
    Always,
    /// It acts, such as a command: it goes again only if the node never
    /// got it.
    IfUnsent,
}

impl Repeat {
    pub(crate) fn when(harmless: bool) -> Self {
        if harmless {
            Self::Always
        } else {
            Self::IfUnsent
        }
    }

    fn allows(self, error: &anyhow::Error) -> bool {
        self == Self::Always || error.chain().any(|cause| cause.is::<Unsent>())
    }
}

impl Online {
    /// `call` on this connection, then on the node's next ones while the
    /// link drops under it and `repeat` allows it.
    pub(super) async fn retrying<T, F: Future<Output = anyhow::Result<T>>>(
        &self,
        repeat: Repeat,
        call: impl Fn(Online) -> F,
    ) -> anyhow::Result<T> {
        let until = Instant::now() + NODE_BACK;
        let mut on = self.clone();
        loop {
            let error = match call(on.clone()).await {
                Ok(reply) => return Ok(reply),
                Err(error) => error,
            };
            match on.again(&error, repeat, until).await {
                Some(back) => on = back,
                None => return Err(error),
            }
        }
    }

    /// The node's next connection to send a call again on, after it failed
    /// with `error` on this one, waiting for it until `until`. `None` when
    /// the call fails as it is: the link is still up, `repeat` does not
    /// allow it, or the node did not come back in time.
    pub(crate) async fn again(
        &self,
        error: &anyhow::Error,
        repeat: Repeat,
        until: Instant,
    ) -> Option<Online> {
        if self.connection.close_reason().is_none() || !repeat.allows(error) {
            return None;
        }
        let app = self.app.upgrade()?;
        let node = self.connection.remote_id();
        let used = Some(self.connection.stable_id());
        let back = app.nodes.reconnected(&node, used, until).await?;
        log::info!(
            "nodes: sending a call again on a new connection; node={} error={error:#}",
            self.name
        );
        Some(back)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn a_call_that_acts_goes_again_only_when_unsent() {
        let unsent = anyhow::Error::new(Unsent(anyhow!("connection lost"))).context("opening");
        let lost = anyhow!("connection lost");
        assert!(Repeat::IfUnsent.allows(&unsent));
        assert!(!Repeat::IfUnsent.allows(&lost));
        assert!(Repeat::Always.allows(&lost));
    }
}
