//! A team's board over the link: the boards this device serves to the
//! teammates who subscribe, and the follow of a teammate's board.

use super::{READ, TeamError, TeamLink};
use beep_proto::team::{Card, Member, TeamMsg, TeamRepo};
use iroh::{EndpointAddr, EndpointId};
use std::time::Duration;
use tokio::{
    sync::mpsc,
    time::{Instant, sleep, timeout_at},
};

/// How long after a teammate's board drops it is dialed again.
const REDIAL: Duration = Duration::from_secs(60);
/// How long a dialed device gets to send the board before the next is
/// tried. Short in tests.
const FIRST_BOARD: Duration = if cfg!(test) {
    Duration::from_secs(1)
} else {
    READ
};
/// The largest message a teammate's gateway may send.
const MAX_FRAME: usize = 1024 * 1024;
/// What a teammate's gateway said of their board.
pub(in crate::team) enum Heard {
    Board(Vec<Card>),
    Change(Card, bool),
    Gone,
}

/// What the follow of one teammate's board heard.
pub(in crate::team) struct Followed {
    pub(in crate::team) repo: TeamRepo,
    pub(in crate::team) mesh: EndpointId,
    pub(in crate::team) heard: Heard,
}

/// Follows `member`'s board of `repo` until stopped: subscribes through
/// the first of their devices that answers, passes on what it hears, and
/// dials again a while after the link drops.
pub(in crate::team) async fn follow(
    link: TeamLink,
    repo: TeamRepo,
    member: Member,
    heard: mpsc::Sender<Followed>,
) {
    loop {
        if let Err(e) = subscribe(&link, &repo, &member, &heard).await {
            log::debug!("the board of {} on {repo} dropped: {e}", member.login);
        }

        let gone = Followed {
            repo: repo.clone(),
            mesh: member.mesh,
            heard: Heard::Gone,
        };
        if heard.send(gone).await.is_err() {
            return;
        }

        sleep(REDIAL).await;
    }
}

/// Subscribes to `member`'s board through each of their devices in turn
/// until one serves it, and passes on what it says until that link drops.
async fn subscribe(
    link: &TeamLink,
    repo: &TeamRepo,
    member: &Member,
    heard: &mpsc::Sender<Followed>,
) -> Result<(), TeamError> {
    let mut failed = TeamError::Unreachable;

    for device in &member.devices {
        match listen(link, repo, member, device, heard).await {
            Ok(()) => return Ok(()),
            Err(e) => failed = e,
        }
    }

    Err(failed)
}

/// Follows `repo`'s board on one of `member`'s devices; an error when the
/// device does not serve it, or not within `FIRST_BOARD`.
async fn listen(
    link: &TeamLink,
    repo: &TeamRepo,
    member: &Member,
    device: &EndpointAddr,
    heard: &mpsc::Sender<Followed>,
) -> Result<(), TeamError> {
    let (mut tx, mut rx) = link.dial(std::slice::from_ref(device)).await?;
    rx.set_max_frame(MAX_FRAME);
    tx.send(&TeamMsg::Subscribe { repo: repo.clone() }).await?;
    let mut served = false;
    let deadline = Instant::now() + FIRST_BOARD;

    loop {
        let next = rx.recv::<TeamMsg>();
        let got = match served {
            true => next.await,
            false => timeout_at(deadline, next)
                .await
                .map_err(|_| TeamError::Refused("the board did not come".to_owned()))?,
        };
        let msg = match got {
            Ok(Some(msg)) => msg,
            _ if served => return Ok(()),
            Ok(None) => return Err(TeamError::Refused("the board was not served".to_owned())),
            Err(e) => return Err(e.into()),
        };
        let said = match msg {
            TeamMsg::Board { repo: of, cards } if of == *repo => Heard::Board(cards),
            TeamMsg::BoardChange {
                repo: of,
                card,
                gone,
            } if of == *repo => Heard::Change(card, gone),
            TeamMsg::Refused { reason } => return Err(TeamError::Refused(reason)),
            _ => continue,
        };
        served = true;
        let followed = Followed {
            repo: repo.clone(),
            mesh: member.mesh,
            heard: said,
        };

        if heard.send(followed).await.is_err() {
            return Ok(());
        }
    }
}

