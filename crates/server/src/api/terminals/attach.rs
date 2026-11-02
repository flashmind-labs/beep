//! `AttachTerminal`: the app follows an interactive terminal live. The
//! server relays the node's output to the app and the app's keys and size
//! to the node, holding nothing itself: QUIC flow control on both streams
//! bounds what is in flight.

use super::{online, state, state_of};
use crate::{
    api::Reply,
    protocol::{
        ApiError, AttachTerminal, MAX_TERMINAL_INPUT, TerminalEvent, TerminalInput, TerminalState,
        node::{
            LinkRx, LinkTx,
            process::TaskId,
            terminal::{Reply as PtyReply, Request as PtyRequest, Size},
        },
    },
    router::{Closed, Cx, Sink, Uplink},
};
use anyhow::Result;

pub(in crate::api) async fn attach_terminal(
    cx: Cx,
    request: AttachTerminal,
    mut uplink: Uplink<TerminalInput>,
    sink: Sink<TerminalEvent>,
) -> Reply<()> {
    let row = cx
        .user
        .terminal(request.id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    if !row.pty {
        sink.send(&TerminalEvent::ViewOnly {}).await?;
        return Ok(());
    }
    let Some(node) = online(&cx, &row.endpoint) else {
        return ended(&sink, TerminalState::Offline {}).await;
    };
    // The number must still be this terminal's: a restarted node hands
    // numbers out again.
    let listed = match node.terminals(&PtyRequest::List {}).await {
        Ok(PtyReply::Terminals { terminals }) => terminals,
        Ok(_) | Err(_) => return ended(&sink, TerminalState::Offline {}).await,
    };
    let now = state_of(&row, &listed);
    if now == (TerminalState::Gone {}) {
        return ended(&sink, now).await;
    }
    let size = Size::clamped(request.cols, request.rows);
    let (mut tx, mut rx) = node
        .attach_terminal(TaskId(row.task), request.from, size)
        .await
        .map_err(|e| ApiError::bad_request(format!("{}: {e:#}", node.name)))?;
    relay(&mut tx, &mut rx, &mut uplink, &sink).await
}

async fn ended(sink: &impl Screen, state: TerminalState) -> Reply<()> {
    sink.show(TerminalEvent::Ended { state }).await?;
    Ok(())
}

/// Where the node's side goes: its attached stream.
pub(super) trait ToNode {
    async fn tell(&mut self, request: &PtyRequest) -> Result<()>;
}

impl ToNode for LinkTx {
    async fn tell(&mut self, request: &PtyRequest) -> Result<()> {
        self.send(request).await
    }
}

/// What the node sends on its attached stream.
pub(super) trait FromNode {
    async fn hear(&mut self) -> Result<Option<PtyReply>>;
}

impl FromNode for LinkRx {
    async fn hear(&mut self) -> Result<Option<PtyReply>> {
        self.recv().await
    }
}

/// What the app sends after its request.
pub(super) trait Keys {
    async fn next_input(&mut self) -> Result<Option<TerminalInput>, ApiError>;
}

impl Keys for Uplink<TerminalInput> {
    async fn next_input(&mut self) -> Result<Option<TerminalInput>, ApiError> {
        self.next().await
    }
}

/// Where the app's events go.
pub(super) trait Screen {
    async fn show(&self, event: TerminalEvent) -> Result<(), Closed>;
}

impl Screen for Sink<TerminalEvent> {
    async fn show(&self, event: TerminalEvent) -> Result<(), Closed> {
        self.send(&event).await
    }
}

/// Relays until the terminal ends, the node goes away or the app
/// finishes its side. Both reads are cancel-safe, so `select!` drops
/// nothing half read.
pub(super) async fn relay(
    to_node: &mut impl ToNode,
    from_node: &mut impl FromNode,
    keys: &mut impl Keys,
    screen: &impl Screen,
) -> Reply<()> {
    loop {
        tokio::select! {
            heard = from_node.hear() => match heard {
                Ok(Some(PtyReply::Output { at, data })) => {
                    screen.show(TerminalEvent::Output { at, data }).await?;
                }
                Ok(Some(PtyReply::Ended { task })) => {
                    return ended(screen, state(task.state)).await;
                }
                // The node no longer has it, such as after a restart.
                Ok(Some(PtyReply::Error { message })) => {
                    log::debug!("attaching a terminal failed: {message}");
                    return ended(screen, TerminalState::Gone {}).await;
                }
                Ok(Some(other)) => log::debug!("ignored {other:?} from an attached terminal"),
                Ok(None) | Err(_) => return ended(screen, TerminalState::Offline {}).await,
            },
            input = keys.next_input() => {
                let request = match input? {
                    Some(TerminalInput::Keys { data }) if data.len() > MAX_TERMINAL_INPUT => {
                        return Err(ApiError::bad_request(format!(
                            "at most {MAX_TERMINAL_INPUT} bytes of keys at a time"
                        )));
                    }
                    Some(TerminalInput::Keys { data }) => PtyRequest::Input { data },
                    Some(TerminalInput::Resize { cols, rows }) => PtyRequest::Resize {
                        size: Size::clamped(cols, rows),
                    },
                    // Detached: closing the node's stream detaches there too.
                    None => return Ok(()),
                };
                if let Err(e) = to_node.tell(&request).await {
                    log::debug!("an attached terminal's node went away: {e:#}");
                    return ended(screen, TerminalState::Offline {}).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::process::{Task, TaskState};
    use serde_bytes::ByteBuf;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    /// The node's side as channels: what the server told it, and what it
    /// sends back.
    struct Node {
        told: mpsc::UnboundedSender<PtyRequest>,
    }

    impl ToNode for Node {
        async fn tell(&mut self, request: &PtyRequest) -> Result<()> {
            Ok(self.told.send(request.clone())?)
        }
    }

    struct NodeOut(mpsc::UnboundedReceiver<PtyReply>);

    impl FromNode for NodeOut {
        async fn hear(&mut self) -> Result<Option<PtyReply>> {
            Ok(self.0.recv().await)
        }
    }

    struct App(mpsc::UnboundedReceiver<TerminalInput>);

    impl Keys for App {
        async fn next_input(&mut self) -> Result<Option<TerminalInput>, ApiError> {
            Ok(self.0.recv().await)
        }
    }

    #[derive(Default)]
    struct Shown(Mutex<Vec<TerminalEvent>>);

    impl Screen for Shown {
        async fn show(&self, event: TerminalEvent) -> Result<(), Closed> {
            self.0.lock().expect("not poisoned in tests").push(event);
            Ok(())
        }
    }

    fn bytes(text: &str) -> ByteBuf {
        ByteBuf::from(text.as_bytes().to_vec())
    }

    #[tokio::test]
    async fn keys_go_to_the_node_and_output_comes_back_until_it_ends() {
        let (told_tx, mut told) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (keys_tx, keys_rx) = mpsc::unbounded_channel();
        keys_tx
            .send(TerminalInput::Keys {
                data: bytes("ls\r"),
            })
            .unwrap();
        keys_tx
            .send(TerminalInput::Resize {
                cols: 120,
                rows: 100_000,
            })
            .unwrap();
        let shown = Shown::default();
        let relayed = tokio::spawn(async move {
            relay(
                &mut Node { told: told_tx },
                &mut NodeOut(out_rx),
                &mut App(keys_rx),
                &shown,
            )
            .await
            .map(|()| shown.0.into_inner().expect("not poisoned in tests"))
        });
        assert_eq!(
            told.recv().await,
            Some(PtyRequest::Input {
                data: bytes("ls\r")
            })
        );
        assert_eq!(
            told.recv().await,
            Some(PtyRequest::Resize {
                size: Size {
                    cols: 120,
                    rows: 500
                }
            })
        );
        out_tx
            .send(PtyReply::Output {
                at: 0,
                data: bytes("README.md\r\n"),
            })
            .unwrap();
        out_tx
            .send(PtyReply::Ended {
                task: Task {
                    id: TaskId(1),
                    command: "bash".into(),
                    state: TaskState::Exited { code: Some(0) },
                },
            })
            .unwrap();
        let shown = relayed.await.unwrap().expect("relays");
        assert_eq!(
            shown,
            vec![
                TerminalEvent::Output {
                    at: 0,
                    data: bytes("README.md\r\n")
                },
                TerminalEvent::Ended {
                    state: TerminalState::Exited { code: Some(0) }
                },
            ]
        );
        drop(keys_tx);
    }

    #[tokio::test]
    async fn the_app_detaching_ends_the_relay_quietly() {
        let (told_tx, _told) = mpsc::unbounded_channel();
        let (_out_tx, out_rx) = mpsc::unbounded_channel();
        let (keys_tx, keys_rx) = mpsc::unbounded_channel();
        drop(keys_tx);
        let shown = Shown::default();
        relay(
            &mut Node { told: told_tx },
            &mut NodeOut(out_rx),
            &mut App(keys_rx),
            &shown,
        )
        .await
        .expect("a detach is no error");
        assert!(shown.0.into_inner().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_node_going_away_reads_as_offline() {
        let (told_tx, _told) = mpsc::unbounded_channel();
        let (out_tx, out_rx) = mpsc::unbounded_channel();
        let (_keys_tx, keys_rx) = mpsc::unbounded_channel();
        drop(out_tx);
        let shown = Shown::default();
        relay(
            &mut Node { told: told_tx },
            &mut NodeOut(out_rx),
            &mut App(keys_rx),
            &shown,
        )
        .await
        .expect("relays");
        assert_eq!(
            shown.0.into_inner().unwrap(),
            vec![TerminalEvent::Ended {
                state: TerminalState::Offline {}
            }]
        );
    }

    #[tokio::test]
    async fn too_many_keys_at_once_are_refused() {
        let (told_tx, mut told) = mpsc::unbounded_channel();
        let (_out_tx, out_rx) = mpsc::unbounded_channel();
        let (keys_tx, keys_rx) = mpsc::unbounded_channel();
        keys_tx
            .send(TerminalInput::Keys {
                data: ByteBuf::from(vec![b'x'; MAX_TERMINAL_INPUT + 1]),
            })
            .unwrap();
        let shown = Shown::default();
        let refused = relay(
            &mut Node { told: told_tx },
            &mut NodeOut(out_rx),
            &mut App(keys_rx),
            &shown,
        )
        .await;
        assert!(matches!(refused, Err(ApiError::BadRequest { .. })));
        assert!(told.try_recv().is_err(), "nothing reached the node");
    }
}
