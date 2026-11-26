//! Signed device operations and their wire representation.

use crate::error::MeshError;
use iroh::{EndpointId, SecretKey, Signature};
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr};

const SIGN_DOMAIN: &[u8] = b"beep-mesh/2 device-op";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum DeviceKind {
    Node,
    Desktop,
}

/// Signed by encoding the decoded value again, so it never gains a field in
/// place: one extra element fails every stored signature. Change it only
/// with a new `SIGN_DOMAIN` and new ALPNs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    #[serde(with = "crate::wire::id")]
    pub id: EndpointId,
    pub name: String,
    pub kind: DeviceKind,
    pub added_at_ms: u64,
}

/// Signed by re-encoding, like `Device`: its variants gain no field in place.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum OpBody {
    Genesis(Device),
    Add(Device),
    Remove {
        #[serde(with = "crate::wire::id")]
        id: EndpointId,
        at_ms: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceOp {
    #[serde(with = "crate::wire::id")]
    pub author: EndpointId,
    pub(super) body: OpBody,
    #[serde(with = "crate::wire::sig")]
    pub(super) sig: Signature,
}

impl DeviceOp {
    fn message(mesh: EndpointId, body: &OpBody) -> Result<Vec<u8>, MeshError> {
        let mut msg = SIGN_DOMAIN.to_vec();
        msg.extend_from_slice(mesh.as_bytes());
        msg.extend(rmp_serde::to_vec(body)?);
        Ok(msg)
    }

    pub(crate) fn sign(key: &SecretKey, mesh: EndpointId, body: OpBody) -> Result<Self, MeshError> {
        let sig = key.sign(&Self::message(mesh, &body)?);
        Ok(Self {
            author: key.public(),
            body,
            sig,
        })
    }

    /// A genesis is signed for the mesh its author starts, any other op for `mesh`.
    pub(super) fn verify(&self, mesh: EndpointId) -> Result<(), MeshError> {
        let mesh = match self.body {
            OpBody::Genesis(_) => self.author,
            _ => mesh,
        };
        self.author
            .verify(&Self::message(mesh, &self.body)?, &self.sig)
            .map_err(|_| MeshError::BadSignature)
    }
}
