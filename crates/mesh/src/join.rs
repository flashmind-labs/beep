//! Joining a mesh. A desktop mints a ticket to bring a device in; the
//! secret in it proves the holder was given the ticket. On `beep-join/2` the
//! joiner shows the secret, and the desktop that minted it spends the
//! secret, signs the `Add` and answers with every op.

use crate::mesh::{AddrNote, Inner};
use crate::wire::{self, FrameReader, MAX_FRAME, MAX_REQUEST};
use crate::{DeviceKind, DeviceOp, Devices, JOIN_ALPN, Key, MeshError};
use beep_proto::lenient;
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use iroh_tickets::{ParseError, Ticket};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};
use std::time::Duration;
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::time::error::Elapsed;
use tokio::time::timeout;
use tracing::debug;

/// The prefix of tickets from before `beep-join/2`.
const OLD_KIND: &str = "beepjoin";

/// Random bytes in a ticket. `Debug` prints none of them.
#[derive(Clone, Serialize, Deserialize)]
pub struct JoinSecret(#[serde(with = "serde_bytes")] [u8; 32]);

impl JoinSecret {
    pub fn generate() -> Self {
        Self(SecretKey::generate().to_bytes())
    }

    /// Compares in constant time.
    pub fn verify(&self, other: &JoinSecret) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Debug for JoinSecret {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("JoinSecret(..)")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinTicket {
    #[serde(with = "crate::wire::id")]
    pub mesh: EndpointId,
    #[serde(with = "crate::wire::addr")]
    pub addr: EndpointAddr,
    pub kind: DeviceKind,
    pub secret: JoinSecret,
    pub expires_ms: u64,
    /// The device leaves the mesh when it stops: the desktop that spends the ticket writes
    /// `temporary/<id>` for it. Last, so a node from before it reads the rest.
    #[serde(default)]
    pub temporary: bool,
}

/// Why ticket text could not be read. Neither case quotes the text.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TicketError {
    /// A ticket from before `beep-join/2`; the desktop mints a new one.
    #[error("this ticket is from an older version; ask for a new one")]
    Old,
    #[error("that is not a join ticket")]
    Unreadable,
}

impl JoinTicket {
    /// Reads the text a desktop shows. Use this rather than `decode_string`,
    /// which cannot tell an old ticket from a mistyped one.
    pub fn parse(text: &str) -> Result<Self, TicketError> {
        let text = text.trim();
        // Base32 includes `2`, so an old ticket can begin like a new one:
        // decide only after the new format failed to read it.
        Self::decode_string(text).map_err(|_| match text.starts_with(OLD_KIND) {
            true => TicketError::Old,
            false => TicketError::Unreadable,
        })
    }

    pub fn expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_ms
    }
}

impl Ticket for JoinTicket {
    const KIND: &'static str = "beepjoin2";

    fn encode_bytes(&self) -> Vec<u8> {
        // Every field is plain data that msgpack always encodes.
        rmp_serde::to_vec(self).expect("msgpack encodes a ticket")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        lenient::from_slice(bytes)
            .map_err(|_| ParseError::verification_failed("the ticket cannot be read"))
    }
}

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
    ticket: &JoinTicket,
    name: &str,
) -> Result<Welcome, MeshError> {
    let work = async {
        let conn = endpoint
            .connect(ticket.addr.clone(), JOIN_ALPN)
            .await
            .map_err(wire::net)?;
        let (mut send, recv) = conn.open_bi().await.map_err(wire::net)?;
        let request = Request {
            secret: ticket.secret.clone(),
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
            if devices.mesh() != ticket.mesh {
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
pub(crate) async fn serve(inner: &Inner, conn: Connection) {
    if inner.is_closed() || !inner.view().devices.is_desktop(inner.me) {
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
        let temporary = Key::Temporary(joiner.to_string());
        if outcome.is_ok() && inner.view().values.contains_key(&temporary) {
            // No receiver is a normal state.
            inner.changes.send(temporary).ok();
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
