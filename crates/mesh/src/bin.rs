//! msgpack writes a byte array element by element unless it is asked for
//! `bin`, which costs a key or a signature up to twice its size. These
//! `serde(with)` modules write them as `bin`, and read either form.

use serde_bytes::ByteBuf;

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
    use crate::Heads;
    use iroh::EndpointId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    struct Origin(#[serde(with = "super::id")] EndpointId);

    pub(crate) fn serialize<S: Serializer>(heads: &Heads, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(heads.iter().map(|(id, seq)| (Origin(*id), seq)))
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Heads, D::Error> {
        let map = std::collections::BTreeMap::<Origin, u64>::deserialize(d)?;
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
