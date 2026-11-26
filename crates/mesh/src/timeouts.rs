use std::time::Duration;

/// How long a network open may take, from the new stream to the node's answer.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// The pause before trying to reach a machine again after a link or stream
/// to it dropped or did not open.
pub const RETRY: Duration = Duration::from_secs(5);
