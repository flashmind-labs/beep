//! `device/1` for the node and its agents: the stream's requests that name a folder or
//! file are checked against what the node's agents may reach before the
//! device code answers them.

use super::{Agents, reach::Access};
use crate::run::device::{self, Served};
use anyhow::Result;
use beep_proto::node::{LinkRx, LinkTx};

pub(crate) async fn serve(tx: LinkTx, rx: LinkRx, agents: &Agents) -> Result<()> {
    let mut stream = Served::new(tx, rx);

    while let Some(request) = stream.next().await? {
        let path = match device::named(&request) {
            Some(named) => Some(agents.reachable(named, Access::Read).await),
            None => None,
        };

        match stream.answer(request, path).await? {
            Some(more) => stream = more,
            None => break,
        }
    }

    Ok(())
}
