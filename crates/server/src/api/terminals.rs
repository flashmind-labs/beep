//! Terminals of released apps. The server reaches no machine: lists are empty and the rest is
//! refused; a desktop keeps terminals over its mesh.

use super::{Reply, own_nodes::retired};
use crate::{
    api::router::{Cx, Sink, Uplink},
    protocol::*,
};

pub(super) async fn list_terminals(_: Cx, _: ListTerminals) -> Reply<Vec<Terminal>> {
    Ok(Vec::new())
}

pub(super) async fn list_all_terminals(_: Cx, _: ListAllTerminals) -> Reply<Vec<Terminal>> {
    Ok(Vec::new())
}

pub(super) async fn start_terminal(_: Cx, _: StartTerminal) -> Reply<Terminal> {
    retired()
}

pub(super) async fn open_terminal(_: Cx, _: OpenTerminal) -> Reply<Terminal> {
    retired()
}

pub(super) async fn get_terminal(_: Cx, _: GetTerminal) -> Reply<TerminalOutput> {
    retired()
}

pub(super) async fn close_terminal(_: Cx, _: CloseTerminal) -> Reply<()> {
    retired()
}

/// Ends at once: the terminal's machine is out of the server's reach.
pub(super) async fn attach_terminal(
    _: Cx,
    _: AttachTerminal,
    _: Uplink<TerminalInput>,
    sink: Sink<TerminalEvent>,
) -> Reply<()> {
    sink.send(&TerminalEvent::Ended {
        state: TerminalState::Offline {},
    })
    .await?;
    Ok(())
}
