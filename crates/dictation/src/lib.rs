//! Dictation without a server: one clip of speech goes to the transcription model the user
//! picked, through flashmind's providers, and comes back as text. Nothing here knows about the
//! desktop: it gets the key, the clip and, for the settings, a folder.

mod clip;
mod error;
mod settings;
mod wav;

pub use clip::{Length, MAX_CLIP, MIN_CLIP, RATE, duration, is_full, judge, max_bytes};
pub use error::DictationError;
pub use settings::{Chosen, Settings, load_settings, save_settings};
pub use wav::wrap_pcm16;

use flashmind::{
    llm::{
        LlmProvider, MistralProvider, OpenAiProvider, OpenRouterProvider, RoutingTable, StreamEvent,
    },
    types::SttRequest,
};
use futures::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};

/// Who transcribes. Each one's key is the one saved for it in the mesh.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::IntoStaticStr,
    strum::EnumString,
    strum::EnumIter,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Provider {
    OpenRouter,
    OpenAi,
    Mistral,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenRouter => "OpenRouter",
            Self::OpenAi => "OpenAI",
            Self::Mistral => "Mistral",
        }
    }

    /// The model used until the user types another. OpenRouter has none: its models are listed.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenRouter => "openai/whisper-1",
            Self::OpenAi => "gpt-4o-mini-transcribe",
            Self::Mistral => "voxtral-mini-latest",
        }
    }
}

/// A transcription model OpenRouter lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
}

/// Transcribes the WAV file `wav` with `model`. `base` replaces the provider's address, for a
/// test server; OpenRouter's address is fixed in flashmind and `base` does not reach it.
pub async fn transcribe(
    provider: Provider,
    base: Option<&str>,
    key: &SecretString,
    model: &str,
    wav: Vec<u8>,
) -> Result<String, DictationError> {
    let name = provider.name();
    let request = SttRequest {
        model: model.to_owned(),
        audio: wav,
        media_type: "audio/wav".to_owned(),
        language: None,
    };
    let mut events = match provider {
        Provider::OpenRouter => {
            OpenRouterProvider::new(key.expose_secret().to_owned()).transcribe(request)
        }
        Provider::OpenAi => OpenAiProvider::new(
            base.map(str::to_owned),
            Some(key.expose_secret().to_owned()),
            RoutingTable::default(),
            false,
            None,
        )
        .map_err(|e| DictationError::from_report(name, &e.to_string()))?
        .transcribe(request),
        Provider::Mistral => {
            let mistral = MistralProvider::new(key.expose_secret().to_owned());
            match base {
                Some(base) => mistral
                    .with_base_url(base)
                    .map_err(|e| DictationError::from_report(name, &e.to_string()))?
                    .transcribe(request),
                None => mistral.transcribe(request),
            }
        }
    };
    let mut text = String::new();
    while let Some(event) = events.next().await {
        match event {
            Ok(StreamEvent::ContentDelta(delta)) => text.push_str(&delta),
            Ok(StreamEvent::Finished(_)) => break,
            Ok(_) => {}
            Err(e) => return Err(DictationError::from_report(name, &e.to_string())),
        }
    }
    Ok(text.trim().to_owned())
}

/// The transcription models OpenRouter lists, by name. OpenRouter gives no price here.
pub async fn openrouter_models(key: &SecretString) -> Result<Vec<ModelChoice>, DictationError> {
    let listed = OpenRouterProvider::new(key.expose_secret().to_owned())
        .list_transcription_models()
        .await
        .map_err(|_| DictationError::Network(Provider::OpenRouter.name()))?;
    let mut models: Vec<ModelChoice> = listed
        .into_iter()
        .map(|m| ModelChoice {
            name: m.name.unwrap_or_else(|| m.id.clone()),
            id: m.id,
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}
