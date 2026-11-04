//! Keeping the link to the server up: the pause between tries, how fast a
//! dead connection is noticed, and waking from sleep. The CLI and the Mac
//! app's node both follow it.

use iroh::endpoint::{IdleTimeout, QuicTransportConfig, VarInt};
use std::time::{Duration, Instant, SystemTime};
use tokio::time::interval;

const FIRST_RETRY: Duration = Duration::from_secs(1);
const LAST_RETRY: Duration = Duration::from_secs(12);
/// The longest wait for an app the user is looking at.
const EAGER_LAST_RETRY: Duration = Duration::from_secs(2);
/// An idle connection is pinged this often, which also keeps NAT mappings
/// open.
const KEEP_ALIVE: Duration = Duration::from_secs(3);
/// A connection that hears nothing for this long is dead: three missed
/// pings. Both ends take the shorter of their two, so the server drops a
/// node that went away as fast.
const IDLE_MS: u32 = 10_000;
/// How often the clocks are compared for a sleep.
const WAKE_CHECK: Duration = Duration::from_secs(5);

/// The pause before each try: from `FIRST_RETRY`, doubling up to
/// `LAST_RETRY`, each shifted by up to a quarter so nodes cut off together
/// do not all come back at once.
pub struct Backoff {
    wait: Duration,
    last: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            wait: FIRST_RETRY,
            last: LAST_RETRY,
        }
    }
}

impl Backoff {
    /// Up to `EAGER_LAST_RETRY`, for the Mac app, whose user is looking at
    /// it.
    pub fn eager() -> Self {
        Self {
            last: EAGER_LAST_RETRY,
            ..Self::default()
        }
    }

    /// The link worked, so the next drop is tried again at once.
    pub fn reset(&mut self) {
        self.wait = FIRST_RETRY;
    }

    pub fn pause(&mut self) -> Duration {
        let wait = self.wait.mul_f64(0.75 + fastrand::f64() / 2.0);
        self.wait = (self.wait * 2).min(self.last);
        wait
    }
}

/// The transport the link connects with: pinged when idle, given up on
/// after `IDLE_MS` of silence.
pub(crate) fn transport() -> QuicTransportConfig {
    QuicTransportConfig::builder()
        .keep_alive_interval(KEEP_ALIVE)
        .max_idle_timeout(Some(IdleTimeout::from(VarInt::from_u32(IDLE_MS))))
        .build()
}

/// Finishes once the machine wakes from a sleep longer than the idle
/// timeout. The server dropped the link by then, but this side's timers
/// stopped with the monotonic clock and would take another idle timeout to
/// notice; the wall clock kept running, and the gap between them tells.
pub(crate) async fn woke() {
    let idle = Duration::from_millis(IDLE_MS.into());
    let mut tick = interval(WAKE_CHECK);
    let mut wall = SystemTime::now();
    let mut mono = Instant::now();
    loop {
        tick.tick().await;
        let (wall_now, mono_now) = (SystemTime::now(), Instant::now());
        // A clock set back is no sleep.
        let passed = wall_now.duration_since(wall).unwrap_or_default();
        if passed > mono_now.duration_since(mono) + idle {
            return;
        }
        (wall, mono) = (wall_now, mono_now);
    }
}
