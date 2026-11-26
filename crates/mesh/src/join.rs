//! Joining a mesh. A desktop mints a ticket to bring a device in; the
//! secret in it proves the holder was given the ticket. On `beep-join/2` the
//! joiner shows the secret, and the desktop that minted it spends the
//! secret, signs the `Add` and answers with every op.

use crate::inner::{AddrNote, Inner};
use crate::ticket::{Invitation, JoinSecret};
use crate::wire::{self, FrameReader, MAX_FRAME, MAX_REQUEST};
use crate::{DeviceOp, Devices, JOIN_ALPN, Key, MeshError};
use iroh::Endpoint;
use iroh::endpoint::Connection;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::error::Elapsed;
use tokio::time::timeout;
use tracing::debug;

const STEP: Duration = Duration::from_secs(15);
/// How long a stranger gets to send its request.
const READ: Duration = Duration::from_secs(3);
const MAX_NAME: usize = 64;

#[derive(Serialize, Deserialize)]
struct Request {
    secret: JoinSecret,
    name: String,
}

#[derive(Serialize, Deserialize)]
enum Reply {
    Welcome {
        ops: Vec<DeviceOp>,
        addrs: Vec<AddrNote>,
    },
    Refused(String),
}

/// A device name as shown to people: no control characters, bounded.
fn clean(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect()
}

pub(crate) struct Welcome {
    pub devices: Devices,
    pub addrs: Vec<AddrNote>,
}

fn elapsed<T>(r: Result<Result<T, MeshError>, Elapsed>) -> Result<T, MeshError> {
    r.map_err(|_| wire::net("timed out"))?
}

/// The joiner's side.
pub(crate) async fn redeem(
    endpoint: &Endpoint,
    invite: &Invitation,
    name: &str,
) -> Result<Welcome, MeshError> {
    let work = async {
        let conn = endpoint
            .connect(invite.addr.clone(), JOIN_ALPN)
            .await
            .map_err(wire::net)?;
        let (mut send, recv) = conn.open_bi().await.map_err(wire::net)?;
        let request = Request {
            secret: invite.secret.clone(),
            name: clean(name),
        };
        wire::send(&mut send, &request).await?;
        send.finish().map_err(wire::net)?;
        let reply = FrameReader::new(recv, MAX_FRAME).recv::<Reply>().await?;
        conn.close(0u32.into(), b"done");
        reply.ok_or_else(|| wire::net("no answer"))
    };

    match elapsed(timeout(STEP, work).await)? {
        Reply::Refused(why) => Err(MeshError::Refused(why)),
        Reply::Welcome { ops, addrs } => {
            let (devices, _) = Devices::from_ops(&ops)?;

            // The connection proved the desktop's id; it must be a desktop of the mesh it sent.
            if invite.mesh.is_some_and(|mesh| mesh != devices.mesh())
                || !devices.is_desktop(invite.addr.id)
            {
                return Err(MeshError::OtherMesh);
            }

            if !devices.is_member(endpoint.id()) {
                return Err(MeshError::Stranger(endpoint.id()));
            }

            Ok(Welcome { devices, addrs })
        }
    }
}

/// The desktop's side. Nodes refuse.
#[tracing::instrument(name = "join", skip_all, fields(joiner = %conn.remote_id().fmt_short()))]
pub(crate) async fn serve(inner: &Inner, conn: Connection) {
    if inner.is_closed() || !inner.view().devices.is_desktop(inner.me()) {
        conn.close(1u32.into(), b"not a desktop");
        return;
    }

    let Ok(_permit) = inner.joins.try_acquire() else {
        conn.close(1u32.into(), b"busy");
        return;
    };
    let joiner = conn.remote_id();
    let work = async {
        let (mut send, recv) = timeout(READ, conn.accept_bi())
            .await
            .map_err(|_| wire::net("no stream in time"))?
            .map_err(wire::net)?;
        let request = timeout(READ, FrameReader::new(recv, MAX_REQUEST).recv::<Request>())
            .await
            .map_err(|_| wire::net("no request in time"))??
            .ok_or_else(|| wire::net("no request"))?;
        let outcome = inner
            .store
            .run(move |core| core.redeem(&request.secret, joiner, &clean(&request.name)))
            .await;
        if outcome.is_ok() {
            for key in [
                Key::Temporary(joiner.to_string()),
                Key::Observer(joiner.to_string()),
            ] {
                if inner.view().values.contains_key(&key) {
                    // No receiver is a normal state.
                    inner.changes.send(key).ok();
                }
            }
        }

        let reply = match outcome {
            Ok(ops) => Reply::Welcome {
                ops,
                addrs: inner.addr_list(),
            },
            Err(MeshError::Refused(why)) => Reply::Refused(why),
            Err(e) => {
                debug!("a join failed here: {e}");
                Reply::Refused("try again later".to_owned())
            }
        };
        wire::send(&mut send, &reply).await?;
        send.finish().map_err(wire::net)?;
        // The joiner closes once it has read the answer.
        conn.closed().await;
        Ok::<(), MeshError>(())
    };

    if let Err(e) = elapsed(timeout(STEP, work).await) {
        debug!("a join ended: {e}");
    }

    conn.close(0u32.into(), b"done");
}
