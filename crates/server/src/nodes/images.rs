//! The pictures a node's coding agent sessions show, kept as the node sends
//! them on one `FollowImages` stream per connection (see
//! `beep_proto::node::agent`), so they show while the machine is offline.

use super::Online;
use crate::{
    protocol::node::agent::{SESSIONS_PROTOCOL, SessionsReply, SessionsRequest},
    store::{AgentImageBytes, AgentImageOf},
    user::User,
};
use anyhow::{Result, bail};

impl Online {
    /// Keeps each picture the node sends until the stream or the
    /// connection ends. A node from before `FollowImages` closes the
    /// stream at once.
    pub(super) async fn keep_images(&self, user: &User) -> Result<()> {
        let (mut tx, mut rx) = self.open_stream(SESSIONS_PROTOCOL).await?;
        tx.send(&SessionsRequest::FollowImages {}).await?;
        let machine_id = self.connection.remote_id().to_string();
        while let Some(reply) = rx.recv::<SessionsReply>().await? {
            let SessionsReply::SessionImage {
                session,
                mime,
                data,
            } = reply
            else {
                bail!("the node answered something other than a picture");
            };
            let of = AgentImageOf {
                machine_id: &machine_id,
                session: &session,
            };
            // One not kept is still read from the node when asked for.
            if let Err(e) = user
                .save_agent_image(of, &AgentImageBytes { mime, data })
                .await
            {
                log::warn!("agent image not kept; session={session} error={e:#}");
            }
        }
        Ok(())
    }
}
