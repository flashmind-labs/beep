//! What the user picked in the model chip: the model and how hard it thinks. One small file next
//! to the chats.

use crate::{AssistantError, Effort};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FILE: &str = "prefs.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// The OpenRouter model id every new prompt runs on; empty until one is picked or used.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: Effort,
}

/// The saved picks; none saved, or a file that does not read, gives the defaults.
pub async fn load_prefs(dir: &Path) -> Prefs {
    let Ok(bytes) = tokio::fs::read(dir.join(FILE)).await else {
        return Prefs::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        log::warn!("assistant prefs not read: {e}");
        Prefs::default()
    })
}

pub async fn save_prefs(dir: &Path, prefs: &Prefs) -> Result<(), AssistantError> {
    tokio::fs::create_dir_all(dir).await?;
    tokio::fs::write(dir.join(FILE), serde_json::to_vec(prefs)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn picks_survive_and_a_bad_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_prefs(dir.path()).await, Prefs::default());
        let prefs = Prefs {
            model: "a/b".into(),
            effort: Effort::High,
        };
        save_prefs(dir.path(), &prefs).await.unwrap();
        assert_eq!(load_prefs(dir.path()).await, prefs);
        tokio::fs::write(dir.path().join(FILE), b"{oops")
            .await
            .unwrap();
        assert_eq!(load_prefs(dir.path()).await, Prefs::default());
    }
}
