//! `beep-team/1` as this node serves it: the team link answers a teammate's
//! messages, and an ask or an answer for a session of this node is delivered
//! here, before the teammate gets its reply. Tools sit above the team link,
//! so the link returns what is to be delivered instead of asking for it.

use super::{Node, team::deliver};
use crate::team::{
    asks::Answer,
    join::Join,
    link::{FIRST_FRAME, READ, TeamError, TeamLink, finish},
};
use beep_proto::{
    node::{LinkRx, LinkTx, link},
    team::{TeamMsg, TeamRepo},
};
use iroh::{
    EndpointId,
    endpoint::Connection,
    protocol::{AcceptError, ProtocolHandler},
};
use std::fmt;
use tokio::{
    select,
    sync::broadcast::error::RecvError,
    time::timeout,
};

/// Serves `beep-team/1` for `node`.
pub struct TeamAlpn(Node);

impl TeamAlpn {
    pub fn new(node: Node) -> Self {
        Self(node)
    }
}

impl fmt::Debug for TeamAlpn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TeamAlpn")
    }
}

impl ProtocolHandler for TeamAlpn {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        if let Err(e) = serve(&self.0, &connection).await {
            log::debug!(
                "a team link from {} ended: {e}",
                connection.remote_id().fmt_short()
            );
        }

        Ok(())
    }
}

async fn serve(node: &Node, connection: &Connection) -> Result<(), TeamError> {
    let team = &node.team;
    let peer = connection.remote_id();
    let (send, recv) = timeout(READ, connection.accept_bi())
        .await
        .map_err(|_| TeamError::Refused("no stream".to_owned()))?
        .map_err(anyhow::Error::from)?;
    let (mut tx, mut rx) = link(send, recv);

    // A stranger's first message is small; a member's may be a whole roster.
    if !team.with(|teams| teams.rosters.knows(peer)) {
        rx.set_max_frame(FIRST_FRAME);
    }

    let first = timeout(READ, rx.recv::<TeamMsg>())
        .await
        .map_err(|_| TeamError::Refused("no message".to_owned()))??;

    match first {
        None => Ok(()),
        Some(TeamMsg::Join {
            secret,
            repo,
            login,
            mesh,
            devices,
        }) => {
            let join = Join {
                secret,
                repo,
                login,
                mesh,
                devices,
            };

            team.admit(peer, join, tx, rx).await
        }
        Some(TeamMsg::Invite { repo, ticket }) => {
            if let Some(refused) = team.invited(peer, repo, ticket) {
                tx.send(&refused).await?;
            }
            finish(tx).await?;
            Ok(())
        }
        Some(first) if team.with(|teams| teams.rosters.knows(peer)) => {
            serve_member(node, peer, first, tx, rx).await
        }
        Some(_) => {
            connection.close(1u32.into(), b"not on a team");
            Ok(())
        }
    }
}

/// Answers a member's messages until they finish the stream.
async fn serve_member(
    node: &Node,
    peer: EndpointId,
    first: TeamMsg,
    mut tx: LinkTx,
    mut rx: LinkRx,
) -> Result<(), TeamError> {
    let mut next = Some(first);

    while let Some(msg) = next {
        if let TeamMsg::Subscribe { repo } = &msg
            && node.team.serves(peer, repo)
        {
            return serve_board(node, peer, repo.clone(), tx, rx).await;
        }

        if let Some(answer) = reply(node, peer, &msg).await {
            tx.send(&answer).await?;
        }
        next = rx.recv().await?;
    }

    finish(tx).await?;
    Ok(())
}

/// What the teammate gets for `msg`: the team link's answer, after the text
/// it took for a session of this node was delivered. A delivery that fails is
/// taken back and refused.
async fn reply(node: &Node, peer: EndpointId, msg: &TeamMsg) -> Option<TeamMsg> {
    match node.team.answer(peer, msg).await {
        Answer::Send(answer) => answer,
        Answer::Deliver(d) => match deliver(node, &d.card, d.delivery, d.words).await {
            Ok(()) => None,
            Err(reason) => {
                node.team.undo(d.undo);
                Some(TeamMsg::Refused { reason })
            }
        },
    }
}

/// Serves `repo`'s board to `peer`, a member's device: the whole board, then
/// each change, until the peer finishes the stream or is no longer on the
/// team, or the board moves to another machine. What else the peer sends
/// is answered as the link answers it.
async fn serve_board(
    node: &Node,
    peer: EndpointId,
    repo: TeamRepo,
    mut tx: LinkTx,
    mut rx: LinkRx,
) -> Result<(), TeamError> {
    let team: &TeamLink = &node.team;
    let watched = team.served(|served| served.boards.watch(&repo));
    let Some((cards, mut changes)) = watched else {
        return finish(tx).await;
    };
    tx.send(&TeamMsg::Board {
        repo: repo.clone(),
        cards,
    })
    .await?;

    loop {
        select! {
            change = changes.recv() => {
                let msg = match change {
                    _ if !team.with(|teams| teams.rosters.allows(peer, &repo)) => break,
                    Ok((card, gone)) => TeamMsg::BoardChange { repo: repo.clone(), card, gone },
                    Err(RecvError::Lagged(_)) => match team.served(|served| served.boards.cards(&repo)) {
                        Some(cards) => TeamMsg::Board { repo: repo.clone(), cards },
                        None => break,
                    },
                    Err(RecvError::Closed) => break,
                };
                tx.send(&msg).await?;
            }
            msg = rx.recv::<TeamMsg>() => match msg? {
                None => break,
                Some(msg) => {
                    if let Some(answer) = reply(node, peer, &msg).await {
                        tx.send(&answer).await?;
                    }
                }
            },
        }
    }

    finish(tx).await
}
