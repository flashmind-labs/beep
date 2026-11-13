//! The pause between tries to reconnect, for the streams the Mac app keeps
//! open to its machines.

use std::time::Duration;

const FIRST_RETRY: Duration = Duration::from_secs(1);
/// The longest wait for an app the user is looking at.
const LAST_RETRY: Duration = Duration::from_secs(2);

/// The pause before each try: from `FIRST_RETRY`, doubling up to
/// `LAST_RETRY`, each shifted by up to a quarter so nodes cut off together
/// do not all come back at once.
pub struct Backoff {
    wait: Duration,
}

impl Backoff {
    /// For the Mac app, whose user is looking at it.
    pub fn eager() -> Self {
        Self { wait: FIRST_RETRY }
    }

    /// The link worked, so the next drop is tried again at once.
    pub fn reset(&mut self) {
        self.wait = FIRST_RETRY;
    }

    pub fn pause(&mut self) -> Duration {
        let wait = self.wait.mul_f64(0.75 + fastrand::f64() / 2.0);
        self.wait = (self.wait * 2).min(LAST_RETRY);
        wait
    }
}
