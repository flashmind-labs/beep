//! What a node's coding agents spend and how much of their plans is left,
//! kept as the node sends them on one `FollowUsage` stream per connection
//! (see `beep_proto::node::agent`), with what sessions spent as the
//! agents' logs on the node tell it.

use super::Online;
use crate::{
    protocol::node::agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
    user::User,
};
use anyhow::{Result, bail};

impl Online {
    /// Keeps what the node reports until the stream or the connection
    /// ends, asking it to read the plans again whenever `refresh_usage`
    /// is called. A node from before `FollowUsage` closes the stream at
    /// once.
    pub(super) async fn keep_usage(&self, user: &User) -> Result<()> {
        let (mut tx, mut rx) = self.open_stream(SESSIONS_PROTOCOL).await?;
        tx.send(&SessionsRequest::FollowUsage { history: true })
            .await?;
        let machine_id = self.connection.remote_id().to_string();
        loop {
            let reply = tokio::select! {
                reply = rx.recv::<SessionsReply>() => match reply? {
                    Some(reply) => reply,
                    None => return Ok(()),
                },
                () = self.usage_refresh.notified() => {
                    tx.send(&SessionsRequest::RefreshPlanUsage {}).await?;
                    continue;
                }
            };
            // One not kept is lost; the next report of a plan or a session
            // replaces it.
            let kept = match reply {
                SessionsReply::TurnUsage { usage } => {
                    user.add_turn_usage(&machine_id, &usage).await
                }
                SessionsReply::SessionSpend { usage } => {
                    user.save_session_spend(&machine_id, &usage).await
                }
                SessionsReply::PlanUsage { usage } => {
                    user.save_plan_usage(&machine_id, &usage).await.map(drop)
                }
                _ => bail!("the node answered something other than usage"),
            };
            if let Err(e) = kept {
                log::warn!("agent usage not kept; node={machine_id} error={e:#}");
            }
        }
    }

    /// Asks the node to read its agents' plans again now, when it follows
    /// usage. It answers on its own time.
    pub(crate) fn refresh_usage(&self) {
        self.usage_refresh.notify_one();
    }
}
