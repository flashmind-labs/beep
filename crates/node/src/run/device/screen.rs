//! What a device shows and how it is followed: the newest screen, the frames
//! and video a platform produces, and the turn of its screen.

use anyhow::Result;
use beep_proto::node::device::Frame;
use futures::{FutureExt, Stream, StreamExt};
use std::{
    sync::{
        Arc, LazyLock,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use strum::FromRepr;
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, sleep_until},
};
use tokio_util::task::AbortOnDropHandle;
use tonic::Status;

/// The shortest interval between starting frames: about 30 a second.
pub(super) const FRAME_GAP: Duration = Duration::from_millis(33);

/// Rung when adb, or a boot or shutdown here, may have changed the list.
pub(super) static CHANGED: LazyLock<watch::Sender<()>> = LazyLock::new(|| watch::Sender::new(()));

/// What a device shows, newest only.
#[derive(Debug, Clone)]
pub(super) enum Screen {
    /// No frame yet.
    Starting,
    Frame(Frame),
    /// The view cannot go on, and why.
    Ended(String),
}

/// Bounded encoder output. Dropping the attachment stops its producer.
pub(super) struct VideoFeed<C> {
    pub(super) packets: mpsc::Receiver<beep_proto::node::device_video::Reply>,
    pub(super) controls: C,
    pub(super) work: AbortOnDropHandle<()>,
}

/// A device being shown: its screen, kept current by `_work`, and where
/// input goes.
pub(super) struct Live<C> {
    pub(super) screen: watch::Receiver<Screen>,
    pub(super) controls: C,
    pub(super) _work: AbortOnDropHandle<()>,
}

impl<C> Live<C> {
    /// The same view, with its controls as `to` makes them.
    pub(super) fn map<D>(self, to: impl FnOnce(C) -> D) -> Live<D> {
        Live {
            screen: self.screen,
            controls: to(self.controls),
            _work: self._work,
        }
    }
}

impl<C> VideoFeed<C> {
    /// The same feed, with its controls as `to` makes them.
    pub(super) fn map<D>(self, to: impl FnOnce(C) -> D) -> VideoFeed<D> {
        VideoFeed {
            packets: self.packets,
            controls: to(self.controls),
            work: self.work,
        }
    }
}

/// Shows what `stream` sends, newest first and at most one frame per
/// `FRAME_GAP`, until it ends, then ends `screen` with `ended`. `show` makes
/// a frame of a message, or `None` when the screen did not change.
pub(super) async fn follow<T>(
    mut stream: impl Stream<Item = Result<T, Status>> + Unpin,
    screen: watch::Sender<Screen>,
    mut show: impl AsyncFnMut(T) -> Result<Option<Frame>>,
    ended: &str,
) {
    let mut next_frame = Instant::now();
    let mut next_shown = next_frame;
    let reason = loop {
        sleep_until(next_frame).await;
        let message = match newest(&mut stream).await {
            Ok(Some(message)) => message,
            Ok(None) => break ended.to_owned(),
            Err(status) => break format!("{ended} ({})", status.message()),
        };

        next_frame = Instant::now() + FRAME_GAP;

        match show(message).await {
            Ok(Some(frame)) => {
                // A fast frame after a slow conversion must not cause a burst.
                sleep_until(next_shown).await;
                screen.send_replace(Screen::Frame(frame));
                next_shown = Instant::now() + FRAME_GAP;
            }
            Ok(None) => {}
            Err(e) => log::warn!("a device frame was not shown: {e:#}"),
        }
    };
    screen.send_replace(Screen::Ended(reason));
}

/// The next message of `stream`, skipping to the newest of those already
/// waiting; `None` once it ended.
async fn newest<T>(
    stream: &mut (impl Stream<Item = Result<T, Status>> + Unpin),
) -> Result<Option<T>, Status> {
    let Some(mut message) = stream.next().await.transpose()? else {
        return Ok(None);
    };

    while let Some(waiting) = stream.next().now_or_never() {
        match waiting.transpose()? {
            Some(newer) => message = newer,
            None => break,
        }
    }

    Ok(Some(message))
}

/// How a device's screen is turned from upright, as its frames show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, FromRepr)]
#[repr(u8)]
pub(super) enum Turn {
    #[default]
    Upright,
    /// The upright screen turned a quarter turn counterclockwise: its top
    /// is at the frame's left.
    Left,
    UpsideDown,
    /// The upright screen turned a quarter turn clockwise: its top is at
    /// the frame's right.
    Right,
}

impl Turn {
    /// Where fractions `x`, `y` of a frame turned this way are on the
    /// upright screen, which devices take touches on.
    pub(super) fn upright(self, x: f32, y: f32) -> (f32, f32) {
        match self {
            Self::Upright => (x, y),
            Self::Left => (1.0 - y, x),
            Self::UpsideDown => (1.0 - x, 1.0 - y),
            Self::Right => (y, 1.0 - x),
        }
    }
}

/// The turn of the newest frame, set as frames come and read by the
/// controls.
#[derive(Debug, Clone, Default)]
pub(super) struct Turned(Arc<AtomicU8>);

impl Turned {
    pub(super) fn get(&self) -> Turn {
        Turn::from_repr(self.0.load(Ordering::Relaxed)).unwrap_or_default()
    }

    pub(super) fn set(&self, turn: Turn) {
        self.0.store(turn as u8, Ordering::Relaxed);
    }
}

/// The point at fractions `x`, `y` of a `width` by `height` screen, each
/// fraction kept within the screen.
pub(super) fn point(x: f32, y: f32, width: f64, height: f64) -> (f64, f64) {
    let fraction = |f: f32| f64::from(if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) });
    (fraction(x) * width, fraction(y) * height)
}
