//! What a mesh's replicated state holds for nodes: the values of the keys
//! `sync/<name>`, `credential/<id>` and `harness/<agent>` (and `flsh`, which
//! is a `FlshSettings`). Desktops write them, nodes read them. Encoded as
//! msgpack maps with named fields, like the node link, so a field may be
//! added with `#[serde(default)]`.

use super::{EnvSecret, ForgejoLogin, harness::HarnessConfig};
use iroh::EndpointId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeMap;

/// `sync/<name>`: the devices that keep a project in step, and the folder
/// the project has on each. A device not listed does not sync it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSpec {
    pub folders: BTreeMap<EndpointId, String>,
    #[serde(default)]
    pub paused: bool,
}

/// `credential/<id>`: one secret every machine of the mesh holds. Its
/// `Debug` prints nothing of the secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Credential {
    /// An env var every process the node starts gets.
    Env(EnvSecret),
    /// A Forgejo or Gitea server teacli signs in to.
    Forgejo(ForgejoLogin),
}

/// A value that is stored in the mesh's state as bytes.
pub trait StateValue: Serialize + DeserializeOwned {
    fn to_bytes(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec_named(self)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        rmp_serde::from_slice(bytes)
    }
}

impl StateValue for SyncSpec {}
impl StateValue for Credential {}
impl StateValue for HarnessConfig {}
impl StateValue for crate::FlshSettings {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretText;
    use iroh::SecretKey;

    #[test]
    fn values_round_trip_and_hide_secrets_in_debug() {
        let id = SecretKey::generate().public();
        let spec = SyncSpec {
            folders: BTreeMap::from([(id, "/srv/project".to_owned())]),
            paused: false,
        };
        assert_eq!(
            SyncSpec::from_bytes(&spec.to_bytes().unwrap()).unwrap(),
            spec
        );
        let credential = Credential::Env(EnvSecret {
            name: "GH_TOKEN".to_owned(),
            value: SecretText::new("ghp_example1234"),
        });
        let back = Credential::from_bytes(&credential.to_bytes().unwrap()).unwrap();
        assert_eq!(back, credential);
        assert!(!format!("{back:?}").contains("1234"));
    }
}
