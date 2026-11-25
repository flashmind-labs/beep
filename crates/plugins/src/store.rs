use std::path::PathBuf;
use std::time::{Duration, Instant};

use log::Level;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::console::emit;
use crate::ident::PluginId;

/// Folder of the plugins folder that holds each plugin's data, one file per id.
pub(crate) const DATA_DIR: &str = ".data";

/// The most a plugin may store: its keys and values as JSON text.
pub(crate) const MAX_BYTES: usize = 256 * 1024;

/// A change waits this long for more before it is written.
const DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub(crate) enum StoreError {
    #[error("beep.store.set needs a JSON value: {0}")]
    NotJson(#[from] serde_json::Error),
    #[error("beep.store is full: it holds at most {} KB", MAX_BYTES / 1024)]
    Full,
}

/// One plugin's persistent values, kept in memory and in `<plugins dir>/.data/<id>.json`.
pub(crate) struct Store {
    id: PluginId,
    path: PathBuf,
    values: Map<String, Value>,
    bytes: usize,
    /// Since when there are changes not yet written.
    dirty_since: Option<Instant>,
}

impl Store {
    /// Reads the plugin's file. A missing file means no values; one that cannot
    /// be read or parsed is logged and read as empty.
    pub(crate) fn load(id: PluginId, path: PathBuf) -> Self {
        let values = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                emit(
                    &id,
                    Level::Warn,
                    &format!("ignoring a corrupt store: {error}"),
                );
                Map::new()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(error) => {
                emit(&id, Level::Warn, &format!("cannot read the store: {error}"));
                Map::new()
            }
        };
        let bytes = values.iter().map(|(key, value)| size(key, value)).sum();

        Self {
            id,
            path,
            values,
            bytes,
            dirty_since: None,
        }
    }

    /// The value as JSON text.
    pub(crate) fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).map(Value::to_string)
    }

    pub(crate) fn set(&mut self, key: &str, json: &str, now: Instant) -> Result<(), StoreError> {
        let value: Value = serde_json::from_str(json)?;
        let old = self.values.get(key).map_or(0, |old| size(key, old));
        let total = (self.bytes - old).saturating_add(size(key, &value));
        if total > MAX_BYTES {
            return Err(StoreError::Full);
        }

        self.bytes = total;
        self.values.insert(key.to_owned(), value);
        self.dirty_since.get_or_insert(now);

        Ok(())
    }

    pub(crate) fn delete(&mut self, key: &str, now: Instant) {
        if let Some(old) = self.values.remove(key) {
            self.bytes -= size(key, &old);
            self.dirty_since.get_or_insert(now);
        }
    }

    /// When the changes are due to be written.
    pub(crate) fn due(&self) -> Option<Instant> {
        self.dirty_since.map(|since| since + DEBOUNCE)
    }

    pub(crate) fn flush_due(&mut self, now: Instant) {
        if self.due().is_some_and(|due| due <= now) {
            self.flush();
        }
    }

    /// Writes a temporary file, then renames it over the real one, so a crash
    /// leaves the old file or the new one. A failure is logged, not retried.
    pub(crate) fn flush(&mut self) {
        if self.dirty_since.take().is_none() {
            return;
        }

        if let Err(error) = self.write() {
            emit(
                &self.id,
                Level::Warn,
                &format!("cannot save the store: {error}"),
            );
        }
    }

    fn write(&self) -> std::io::Result<()> {
        let text = serde_json::to_vec(&self.values).map_err(std::io::Error::other)?;
        let temporary = self.path.with_extension("json.tmp");
        if let Some(folder) = self.path.parent() {
            std::fs::create_dir_all(folder)?;
        }

        std::fs::write(&temporary, text)?;
        std::fs::rename(&temporary, &self.path).inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
    }
}

fn size(key: &str, value: &Value) -> usize {
    key.len() + value.to_string().len()
}

#[cfg(test)]
#[path = "tests/store.rs"]
mod tests;
