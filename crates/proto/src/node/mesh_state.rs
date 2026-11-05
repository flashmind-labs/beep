//! What a mesh's replicated state holds for nodes: the values of the keys
//! `sync/<name>`, `credential/<id>`, `harness/<agent>` and `terminal/<id>` (and `flsh`, which
//! is a `FlshSettings`). Desktops write them, nodes read them. Encoded as
//! compact msgpack, read by position, like the node link: a field is added
//! at the end with `#[serde(default)]`.

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

/// `terminal/<id>`: a terminal a desktop keeps running on a machine. The
/// node knows its tasks by number; this says which one is which, so every
/// desktop lists the same terminals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRecord {
    /// `org/repo`, or empty for a terminal of no project.
    pub project: String,
    /// The machine it runs on.
    pub machine: EndpointId,
    pub task: u32,
    /// An interactive terminal, not a view-only task.
    pub pty: bool,
    pub name: String,
    /// What it ran; tells apart a restarted node's task under the same number.
    pub command: String,
    pub cwd: String,
    /// Milliseconds since the Unix epoch.
    pub started_at: u64,
}

/// A value that is stored in the mesh's state as bytes.
pub trait StateValue: Serialize + DeserializeOwned {
    fn to_bytes(&self) -> Result<Vec<u8>, rmp_serde::encode::Error> {
        rmp_serde::to_vec(self)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, rmp_serde::decode::Error> {
        crate::lenient::from_slice(bytes)
    }
}

impl StateValue for SyncSpec {}
impl StateValue for Credential {}
impl StateValue for TerminalRecord {}
impl StateValue for HarnessConfig {}
impl StateValue for crate::FlshSettings {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretText;
    use iroh::SecretKey;

    #[test]
    fn a_value_with_a_field_from_a_newer_build_is_read() {
        #[derive(Serialize)]
        struct NewerSpec {
            folders: BTreeMap<EndpointId, String>,
            paused: bool,
            added_later: u8,
        }
        let id = SecretKey::generate().public();
        let sent = NewerSpec {
            folders: BTreeMap::from([(id, "/srv/project".to_owned())]),
            paused: true,
            added_later: 1,
        };
        let bytes = rmp_serde::to_vec(&sent).unwrap();
        let spec = SyncSpec::from_bytes(&bytes).unwrap();
        assert!(spec.paused);
        assert_eq!(spec.folders.len(), 1);
        // A value from an older build, which wrote maps, still reads.
        let named = rmp_serde::to_vec_named(&sent).unwrap();
        assert!(SyncSpec::from_bytes(&named).unwrap().paused);
    }

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
