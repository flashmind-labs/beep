//! Length-delimited msgpack frames on a stream: a four byte big-endian
//! length, then the compact (positional) msgpack body. A peer's declared
//! length is checked before any buffer is allocated.
//!
//! msgpack writes a byte array element by element unless it is asked for
//! `bin`, which costs a key or a signature up to twice its size. The
//! `serde(with)` modules below write them as `bin`, and read either form.

use crate::MeshError;
use beep_proto::lenient;
use bytes::{Bytes, BytesMut};
use futures::StreamExt;
use iroh::endpoint::{RecvStream, SendStream};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_bytes::ByteBuf;
use std::fmt::Display;
use std::io::ErrorKind;
use std::time::Duration;
use tokio::time::timeout;
use tokio_util::codec::{Encoder, FramedRead, LengthDelimitedCodec};

/// The largest frame either side accepts.
pub(crate) const MAX_FRAME: usize = 4 * 1024 * 1024;
/// The largest frame a stranger may send, such as a join request.
pub(crate) const MAX_REQUEST: usize = 4 * 1024;
/// A peer that takes this long to accept a frame is stalled.
const WRITE: Duration = Duration::from_secs(10);

pub(crate) fn net(e: impl Display) -> MeshError {
    MeshError::Net(e.to_string())
}

fn codec(max: usize) -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(max)
        .new_codec()
}

pub(crate) async fn send<T: Serialize>(
    stream: &mut SendStream,
    value: &T,
) -> Result<(), MeshError> {
    let body = rmp_serde::to_vec(value)?;
    let mut out = BytesMut::with_capacity(body.len() + 4);
    codec(MAX_FRAME)
        .encode(Bytes::from(body), &mut out)
        .map_err(|_| MeshError::TooLarge)?;
    timeout(WRITE, stream.write_all(&out))
        .await
        .map_err(|_| net("write timed out"))?
        .map_err(net)
}

/// Reads frames from one stream. It keeps what it read past a frame, so one
/// reader serves the stream for its whole life.
pub(crate) struct FrameReader(FramedRead<RecvStream, LengthDelimitedCodec>);

impl FrameReader {
    /// A reader that refuses frames longer than `max`.
    pub(crate) fn new(stream: RecvStream, max: usize) -> Self {
        Self(FramedRead::new(stream, codec(max)))
    }

    /// The next frame, or `None` when the peer finished the stream cleanly
    /// between frames.
    pub(crate) async fn recv<T: DeserializeOwned>(&mut self) -> Result<Option<T>, MeshError> {
        match self.0.next().await {
            None => Ok(None),
            Some(Ok(body)) => Ok(Some(lenient::from_slice(&body)?)),
            Some(Err(e)) if e.kind() == ErrorKind::InvalidData => Err(MeshError::TooLarge),
            Some(Err(e)) => Err(net(e)),
        }
    }
}

/// An `EndpointId` as 32 bytes of `bin`.
pub(crate) mod id {
    use super::ByteBuf;
    use iroh::EndpointId;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<S: Serializer>(id: &EndpointId, s: S) -> Result<S::Ok, S::Error> {
        serde_bytes::Bytes::new(id.as_bytes()).serialize(s)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<EndpointId, D::Error> {
        EndpointId::try_from(ByteBuf::deserialize(d)?.as_slice()).map_err(Error::custom)
    }
}

/// A `Signature` as 64 bytes of `bin`.
pub(crate) mod sig {
    use super::ByteBuf;
    use iroh::Signature;
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(crate) fn serialize<S: Serializer>(sig: &Signature, s: S) -> Result<S::Ok, S::Error> {
        serde_bytes::Bytes::new(&sig.to_bytes()).serialize(s)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Signature, D::Error> {
        Signature::try_from(ByteBuf::deserialize(d)?.as_slice()).map_err(Error::custom)
    }
}

/// Heads, a map from origin to sequence number, with `bin` keys.
pub(crate) mod heads {
    use crate::state::Heads;
    use iroh::EndpointId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    struct Origin(#[serde(with = "super::id")] EndpointId);

    pub(crate) fn serialize<S: Serializer>(heads: &Heads, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(heads.iter().map(|(id, seq)| (Origin(*id), seq)))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Heads, D::Error> {
        let map = BTreeMap::<Origin, u64>::deserialize(d)?;
        Ok(map.into_iter().map(|(Origin(id), seq)| (id, seq)).collect())
    }
}

/// An `EndpointAddr` with its id as `bin`.
pub(crate) mod addr {
    use iroh::{EndpointAddr, EndpointId, TransportAddr};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeSet;

    #[derive(Serialize, Deserialize)]
    struct Compact {
        #[serde(with = "super::id")]
        id: EndpointId,
        addrs: BTreeSet<TransportAddr>,
    }

    pub(crate) fn serialize<S: Serializer>(addr: &EndpointAddr, s: S) -> Result<S::Ok, S::Error> {
        Compact {
            id: addr.id,
            addrs: addr.addrs.clone(),
        }
        .serialize(s)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<EndpointAddr, D::Error> {
        let Compact { id, addrs } = Compact::deserialize(d)?;
        Ok(EndpointAddr { id, addrs })
    }
}
