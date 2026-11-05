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
    /// The preferred upstream provider; it belongs to one model.
    #[serde(default)]
    pub provider: Option<ProviderPref>,
}

/// An upstream provider picked for one model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderPref {
    pub model: String,
    /// The provider's tag, which OpenRouter takes in `provider.order`.
    pub tag: String,
}

impl Prefs {
    /// The provider to prefer when running `model`: the saved one only if it was picked for it.
    pub fn provider_for(&self, model: &str) -> Option<&str> {
        self.provider
            .as_ref()
            .filter(|pick| pick.model == model)
            .map(|pick| pick.tag.as_str())
    }

    /// Picks the model, which drops a provider chosen for another one.
    pub fn set_model(&mut self, model: String) {
        if self
            .provider
            .as_ref()
            .is_some_and(|pick| pick.model != model)
        {
            self.provider = None;
        }
        self.model = model;
    }
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
            provider: Some(ProviderPref {
                model: "a/b".into(),
                tag: "deepinfra".into(),
            }),
        };
        save_prefs(dir.path(), &prefs).await.unwrap();
        assert_eq!(load_prefs(dir.path()).await, prefs);
        tokio::fs::write(dir.path().join(FILE), b"{oops")
            .await
            .unwrap();
        assert_eq!(load_prefs(dir.path()).await, Prefs::default());
    }

    #[test]
    fn a_provider_belongs_to_its_model() {
        let mut prefs = Prefs {
            model: "a/b".into(),
            provider: Some(ProviderPref {
                model: "a/b".into(),
                tag: "deepinfra".into(),
            }),
            ..Prefs::default()
        };
        assert_eq!(prefs.provider_for("a/b"), Some("deepinfra"));
        assert_eq!(prefs.provider_for("c/d"), None);
        prefs.set_model("a/b".into());
        assert_eq!(prefs.provider_for("a/b"), Some("deepinfra"));
        prefs.set_model("c/d".into());
        assert_eq!(prefs.provider_for("c/d"), None);
        assert_eq!(prefs.provider, None);
    }

    #[tokio::test]
    async fn an_old_file_without_a_provider_reads() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join(FILE), br#"{"model":"a/b","effort":"low"}"#)
            .await
            .unwrap();
        let prefs = load_prefs(dir.path()).await;
        assert_eq!(prefs.model, "a/b");
        assert_eq!(prefs.provider, None);
    }
}
