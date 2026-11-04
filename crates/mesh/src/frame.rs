//! Length-prefixed postcard frames on a stream. A peer's declared length is
//! checked before any buffer is allocated.

use crate::MeshError;
use iroh::endpoint::{ReadExactError, RecvStream, SendStream};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Display;
use std::time::Duration;
use tokio::time::timeout;

/// The largest frame either side accepts.
pub(crate) const MAX_FRAME: usize = 4 * 1024 * 1024;
/// The largest frame a stranger may send, such as a join request.
pub(crate) const MAX_REQUEST: usize = 4 * 1024;
/// A peer that takes this long to accept a frame is stalled.
const WRITE: Duration = Duration::from_secs(10);

pub(crate) fn net(e: impl Display) -> MeshError {
    MeshError::Net(e.to_string())
}

pub(crate) async fn send<T: Serialize>(
    stream: &mut SendStream,
    value: &T,
) -> Result<(), MeshError> {
    let body = postcard::to_stdvec(value)?;
    if body.len() > MAX_FRAME {
        return Err(MeshError::TooLarge);
    }
    let len = u32::try_from(body.len()).map_err(|_| MeshError::TooLarge)?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend(len.to_be_bytes());
    out.extend(body);
    timeout(WRITE, stream.write_all(&out))
        .await
        .map_err(|_| net("write timed out"))?
        .map_err(net)
}

/// The next frame, or `None` when the peer finished the stream cleanly
/// between frames.
pub(crate) async fn recv<T: DeserializeOwned>(
    stream: &mut RecvStream,
    max: usize,
) -> Result<Option<T>, MeshError> {
    let mut head = [0u8; 4];
    match stream.read_exact(&mut head).await {
        Ok(()) => {}
        Err(ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(e) => return Err(net(e)),
    }
    let len = usize::try_from(u32::from_be_bytes(head)).map_err(|_| MeshError::TooLarge)?;
    if len > max {
        return Err(MeshError::TooLarge);
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.map_err(net)?;
    Ok(Some(postcard::from_bytes(&body)?))
}
