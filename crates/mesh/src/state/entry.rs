use super::Key;
use crate::MeshError;
use iroh::{EndpointId, SecretKey, Signature};
use serde::{Deserialize, Serialize};

const SIGN_DOMAIN: &[u8] = b"beep-mesh/2 state-entry";

/// One signed write. `None` is a delete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: Key,
    #[serde(with = "serde_bytes")]
    pub value: Option<Vec<u8>>,
    pub at_ms: u64,
    #[serde(with = "crate::wire::id")]
    pub origin: EndpointId,
    pub seq: u64,
    #[serde(with = "crate::wire::sig")]
    sig: Signature,
}

/// What an entry's signature covers, in the order of its fields. Never gains
/// a field without a new `SIGN_DOMAIN` and new ALPNs.
#[derive(Serialize)]
struct Signed<'a> {
    key: &'a Key,
    #[serde(with = "serde_bytes")]
    value: Option<&'a [u8]>,
    at_ms: u64,
    #[serde(with = "crate::wire::id")]
    origin: EndpointId,
    seq: u64,
}

impl Entry {
    fn message(
        mesh: EndpointId,
        key: &Key,
        value: &Option<Vec<u8>>,
        at_ms: u64,
        origin: EndpointId,
        seq: u64,
    ) -> Result<Vec<u8>, MeshError> {
        let mut msg = SIGN_DOMAIN.to_vec();
        msg.extend_from_slice(mesh.as_bytes());
        msg.extend(rmp_serde::to_vec(&Signed {
            key,
            value: value.as_deref(),
            at_ms,
            origin,
            seq,
        })?);
        Ok(msg)
    }

    pub(crate) fn sign(
        secret: &SecretKey,
        mesh: EndpointId,
        key: Key,
        value: Option<Vec<u8>>,
        at_ms: u64,
        seq: u64,
    ) -> Result<Entry, MeshError> {
        let origin = secret.public();
        let sig = secret.sign(&Self::message(mesh, &key, &value, at_ms, origin, seq)?);
        Ok(Entry {
            key,
            value,
            at_ms,
            origin,
            seq,
            sig,
        })
    }

    pub(super) fn verify(&self, mesh: EndpointId) -> Result<(), MeshError> {
        let msg = Self::message(
            mesh,
            &self.key,
            &self.value,
            self.at_ms,
            self.origin,
            self.seq,
        )?;
        self.origin
            .verify(&msg, &self.sig)
            .map_err(|_| MeshError::BadSignature)
    }
}
