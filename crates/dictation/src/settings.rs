//! What the user picked for dictation: a provider and a model. One small file on this Mac.

use crate::Provider;
use serde::{Deserialize, Serialize};
use std::{io, path::Path};

const FILE: &str = "dictation.json";

/// The saved pick. With no provider, dictation is off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub provider: Option<Provider>,
    /// Empty means the provider's default.
    #[serde(default)]
    pub model: String,
}

/// What a press of the key transcribes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub provider: Provider,
    pub model: String,
}

impl Settings {
    /// The provider and model to use, or `None` while dictation is off.
    pub fn chosen(&self) -> Option<Chosen> {
        let provider = self.provider?;
        let typed = self.model.trim();
        let model = if typed.is_empty() {
            provider.default_model()
        } else {
            typed
        };
        Some(Chosen {
            provider,
            model: model.to_owned(),
        })
    }
}

/// The saved pick; none saved, or a file that does not read, is off.
pub async fn load_settings(dir: &Path) -> Settings {
    let Ok(bytes) = tokio::fs::read(dir.join(FILE)).await else {
        return Settings::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub async fn save_settings(dir: &Path, settings: &Settings) -> io::Result<()> {
    tokio::fs::create_dir_all(dir).await?;
    let bytes = serde_json::to_vec(settings).map_err(io::Error::other)?;
    tokio::fs::write(dir.join(FILE), bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_provider_dictation_is_off() {
        assert_eq!(Settings::default().chosen(), None);
        let only_a_model = Settings {
            provider: None,
            model: "x".into(),
        };
        assert_eq!(only_a_model.chosen(), None);
    }

    #[test]
    fn an_empty_model_is_the_default_and_a_typed_one_wins() {
        let mut settings = Settings {
            provider: Some(Provider::Mistral),
            model: "  ".into(),
        };
        let chosen = settings.chosen().expect("a provider is picked");
        assert_eq!(chosen.model, "voxtral-mini-latest");
        settings.model = " voxtral-mini-2507 ".into();
        assert_eq!(
            settings.chosen().expect("picked").model,
            "voxtral-mini-2507"
        );
    }

    #[tokio::test]
    async fn the_pick_survives_and_a_bad_file_is_off() {
        let dir = tempfile::tempdir().expect("a temp dir");
        assert_eq!(load_settings(dir.path()).await, Settings::default());
        let settings = Settings {
            provider: Some(Provider::OpenAi),
            model: "gpt-4o-transcribe".into(),
        };
        save_settings(dir.path(), &settings).await.expect("saved");
        assert_eq!(load_settings(dir.path()).await, settings);
        tokio::fs::write(dir.path().join(FILE), b"{oops")
            .await
            .expect("written");
        assert_eq!(load_settings(dir.path()).await, Settings::default());
    }
}
