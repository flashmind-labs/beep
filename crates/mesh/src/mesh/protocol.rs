//! Inbound mesh and pairing protocol handlers.

use super::Inner;
use crate::link;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;

/// Serves `beep-peer/2`: members only.
#[derive(Clone)]
pub struct MeshProtocol {
    pub(super) inner: Arc<Inner>,
}

impl Debug for MeshProtocol {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("MeshProtocol")
    }
}

impl ProtocolHandler for MeshProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let peer = connection.remote_id();

        if self.inner.is_closed() {
            connection.close(1u32.into(), b"closed");
            return Ok(());
        }

        if !self.inner.is_member(peer) {
            if self.inner.view().devices.is_removed(peer) {
                link::tell_removed(&self.inner, connection).await;
            } else {
                connection.close(1u32.into(), b"not a member");
            }

            return Ok(());
        }

        link::serve(self.inner.clone(), connection, peer).await;
        Ok(())
    }
}

/// Serves `beep-join/2`: desktops only.
#[derive(Clone)]
pub struct JoinProtocol {
    pub(super) inner: Arc<Inner>,
}

impl Debug for JoinProtocol {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("JoinProtocol")
    }
}

impl ProtocolHandler for JoinProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        crate::join::serve(&self.inner, connection).await;
        Ok(())
    }
}
