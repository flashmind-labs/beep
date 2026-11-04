//! Follows a node's agent events, reopening the stream after failures.

use super::{LinkOrder, MAX_BAD_MESSAGES, wake::Reporter};
use crate::{
    nodes::{News, Watched},
    protocol::node::{
        LinkRx, LinkTx,
        agent::{Event, Status},
    },
};
use anyhow::Result;
use std::collections::VecDeque;
use tokio::sync::{mpsc, oneshot, watch};

/// Why a stream did not open.
pub(super) enum Opening {
    /// The link failed before the node took the stream; nothing was sent
    /// on it.
    Link(anyhow::Error),
    /// The node does not serve the protocol, or said no.
    Refused(anyhow::Error),
}

/// A try of a sessions request that failed, and how.
pub(super) struct Failed {
    pub(super) kind: Failure,
    pub(super) error: anyhow::Error,
}

impl Failed {
    pub(super) fn new(kind: Failure, error: anyhow::Error) -> Self {
        Self { kind, error }
    }
}

/// How a sessions request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    /// The request never reached the node: the stream did not open or the
    /// request could not be written.
    Unsent,
    /// The connection closed after the request went out; the node may
    /// have acted on it.
    Dropped,
    /// No answer in time on a connection still open.
    Silent,
    /// The node answered, refused or closed the stream itself.
    Answered,
}

/// Whether a failed request goes again on the node's next connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Retry {
    No,
    /// Only on a newer connection the node has made already.
    IfReplaced,
    /// On the node's next connection, waiting for it up to
    /// `RECONNECT_WAIT`.
    AfterReconnect,
}

impl Failure {
    /// A request the node never got may always go again; a read lost with
    /// its link may too. A read on a live link that went unanswered goes
    /// again only when the node has reconnected meanwhile: on the same
    /// link it would likely hang again.
    pub(super) fn retry(self, read: bool) -> Retry {
        match self {
            Self::Unsent => Retry::AfterReconnect,
            Self::Dropped if read => Retry::AfterReconnect,
            Self::Silent if read => Retry::IfReplaced,
            Self::Dropped | Self::Silent | Self::Answered => Retry::No,
        }
    }
}

/// Reads a session stream into `state` and writes the orders, until either ends.
pub(super) async fn follow(
    mut tx: LinkTx,
    mut rx: LinkRx,
    mut orders: mpsc::Receiver<LinkOrder>,
    state: watch::Sender<Watched>,
    reporter: Reporter,
) {
    let mut acks: VecDeque<oneshot::Sender<Result<(), String>>> = VecDeque::new();
    let mut bad = 0;
    loop {
        tokio::select! {
            order = orders.recv() => {
                let Some(LinkOrder { request, done }) = order else {
                    return;
                };
                if let Err(e) = tx.send(&request).await {
                    log::debug!("agent stream lost: {e:#}");
                    break;
                }
                acks.push_back(done);
            }
            event = rx.recv::<Event>() => match event {
                Ok(Some(event)) => {
                    bad = 0;
                    if let Event::Ack { error } = event {
                        if let Some(done) = acks.pop_front() {
                            // The caller may have stopped waiting.
                            let _ = done.send(error.map_or(Ok(()), Err));
                        }
                        continue;
                    }
                    let ended = matches!(event, Event::Ended { .. });
                    let news = News::of(&event);
                    state.send_modify(|watched| apply(watched, event));
                    if let Some(news) = news {
                        reporter.tell(news);
                    }
                    if ended {
                        return;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    log::debug!("agent stream message skipped: {e:#}");
                    bad += 1;
                    if bad >= MAX_BAD_MESSAGES {
                        break;
                    }
                }
            },
        }
    }
    state.send_modify(|watched| watched.lost = true);
}

pub(super) fn apply(watched: &mut Watched, event: Event) {
    let info = &mut watched.info;
    match event {
        Event::State { state } => {
            if state.status != Status::Asking {
                info.question = None;
            }
            info.state = state;
        }
        Event::Question { question } => {
            info.question = Some(question);
            info.state.status = Status::Asking;
        }
        Event::TurnDone {
            turn,
            stop_reason,
            last_message,
        } => {
            info.turns = turn;
            info.last_message = last_message;
            watched.last_stop = Some(stop_reason);
        }
        Event::Ended { reason } => {
            info.state.status = Status::Ended;
            info.question = None;
            watched.ended = Some(reason);
        }
        other => log::debug!("agent stream sent something unhandled: {other:?}"),
    }
}
