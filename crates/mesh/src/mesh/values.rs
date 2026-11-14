//! Access to replicated values and their change notifications.

use super::Mesh;
use crate::{Key, MeshError};
use tokio::sync::{broadcast, watch};

impl Mesh {
    /// Writes a value, durable on return. Desktops only.
    pub async fn set(&self, key: Key, value: Option<Vec<u8>>) -> Result<(), MeshError> {
        let touched = key.clone();
        self.inner
            .store
            .run(move |core| core.set(key, value))
            .await?;
        // No receiver is a normal state.
        self.inner.changes.send(touched).ok();
        Ok(())
    }

    /// Writes `value` only if this mesh never held `key`: a key it holds, or held and deleted,
    /// keeps its state. Returns whether it wrote. Desktops only.
    pub async fn set_if_new(&self, key: Key, value: Vec<u8>) -> Result<bool, MeshError> {
        let touched = key.clone();
        let wrote = self
            .inner
            .store
            .run(move |core| core.set_if_new(key, value))
            .await?;

        if wrote {
            self.inner.changes.send(touched).ok();
        }

        Ok(wrote)
    }

    pub fn get(&self, key: &Key) -> Option<Vec<u8>> {
        self.inner.view().values.get(key).cloned()
    }

    /// Live keys whose text form starts with `prefix`, such as `project/`.
    pub fn scan(&self, prefix: &str) -> Vec<(Key, Vec<u8>)> {
        self.inner
            .view()
            .values
            .iter()
            .filter(|(key, _)| key.to_string().starts_with(prefix))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Whether any key whose text form starts with `prefix` was ever
    /// written, even if deleted since.
    pub fn has_held(&self, prefix: &str) -> bool {
        self.inner
            .view()
            .held
            .iter()
            .any(|key| key.to_string().starts_with(prefix))
    }

    /// Keys that changed, written here or taken from a peer. A receiver that
    /// falls behind gets `Lagged` and must rescan; subscribe before the first
    /// `scan` so nothing falls between the two.
    pub fn changes(&self) -> broadcast::Receiver<Key> {
        self.inner.changes.subscribe()
    }

    /// Rises whenever the device list, the state or who is online changes.
    pub fn updates(&self) -> watch::Receiver<u64> {
        self.inner.rev.subscribe()
    }
}
