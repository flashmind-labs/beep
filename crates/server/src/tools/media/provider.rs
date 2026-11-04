//! Which services make what, and the user's keys for them.

use crate::{protocol::ApiProvider, store::ApiKey};
use secrecy::SecretString;
use std::{collections::HashMap, time::Duration};

/// A service that makes media with the user's own key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumString, strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub(crate) enum MediaProvider {
    OpenRouter,
    OpenAi,
    Venice,
    Meshy,
}

impl MediaProvider {
    /// The slug the model names it by, the same as its API key's.
    pub(crate) fn slug(self) -> &'static str {
        self.into()
    }

    /// What the user knows it as.
    pub(crate) fn name(self) -> &'static str {
        self.api().name()
    }

    fn api(self) -> ApiProvider {
        match self {
            Self::OpenRouter => ApiProvider::OpenRouter,
            Self::OpenAi => ApiProvider::OpenAi,
            Self::Venice => ApiProvider::Venice,
            Self::Meshy => ApiProvider::Meshy,
        }
    }

    fn from_api(provider: ApiProvider) -> Option<Self> {
        match provider {
            ApiProvider::OpenRouter => Some(Self::OpenRouter),
            ApiProvider::OpenAi => Some(Self::OpenAi),
            ApiProvider::Venice => Some(Self::Venice),
            ApiProvider::Meshy => Some(Self::Meshy),
            _ => None,
        }
    }
}

/// What a tool makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaKind {
    Image,
    Video,
    Model3d,
}

impl MediaKind {
    pub(crate) const ALL: [Self; 3] = [Self::Image, Self::Video, Self::Model3d];

    pub(crate) fn tool(self) -> &'static str {
        match self {
            Self::Image => "image_gen",
            Self::Video => "video_gen",
            Self::Model3d => "3d_gen",
        }
    }

    /// The kind a tool name is, in either case.
    pub(crate) fn from_tool(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.tool().eq_ignore_ascii_case(name))
    }

    /// The action on an approval card.
    pub(crate) fn action(self) -> &'static str {
        match self {
            Self::Image => "make an image",
            Self::Video => "make a video",
            Self::Model3d => "make a 3D model",
        }
    }

    /// The services that make it, the default first.
    pub(super) fn providers(self) -> &'static [MediaProvider] {
        match self {
            Self::Image => &[
                MediaProvider::OpenRouter,
                MediaProvider::OpenAi,
                MediaProvider::Venice,
            ],
            // OpenAI's video API (Sora) was shut down.
            Self::Video => &[MediaProvider::OpenRouter, MediaProvider::Venice],
            Self::Model3d => &[MediaProvider::Meshy],
        }
    }

    /// The model `provider` uses when the call names none, which may differ `from_image`.
    pub(crate) fn default_model(
        self,
        provider: MediaProvider,
        from_image: bool,
    ) -> Option<&'static str> {
        Some(match (self, provider, from_image) {
            (Self::Image, MediaProvider::OpenRouter, _) => "google/gemini-3.1-flash-image-preview",
            (Self::Image, MediaProvider::OpenAi, _) => "gpt-image-2",
            (Self::Image, MediaProvider::Venice, false) => "qwen-image-2",
            (Self::Image, MediaProvider::Venice, true) => "qwen-image-2-edit",
            (Self::Video, MediaProvider::OpenRouter, _) => "google/veo-3.1",
            (Self::Video, MediaProvider::Venice, false) => "wan-2-7-text-to-video",
            (Self::Video, MediaProvider::Venice, true) => "wan-2-7-image-to-video",
            (Self::Model3d, MediaProvider::Meshy, _) => "latest",
            _ => return None,
        })
    }

    /// How long one job may take, polling included.
    pub(super) fn limit(self) -> Duration {
        match self {
            Self::Image => Duration::from_secs(3 * 60),
            Self::Video => Duration::from_secs(15 * 60),
            Self::Model3d => Duration::from_secs(20 * 60),
        }
    }
}

/// The user's keys for the media services.
#[derive(Default)]
pub(crate) struct Keys(HashMap<MediaProvider, SecretString>);

impl Keys {
    /// The media keys among the user's API keys.
    pub(crate) fn new(keys: Vec<ApiKey>) -> Self {
        Self(
            keys.into_iter()
                .filter_map(|key| Some((MediaProvider::from_api(key.provider)?, key.secret)))
                .collect(),
        )
    }

    /// The services the user has a key for that make `kind`, the default first.
    pub(crate) fn providers(&self, kind: MediaKind) -> Vec<MediaProvider> {
        kind.providers()
            .iter()
            .copied()
            .filter(|provider| self.0.contains_key(provider))
            .collect()
    }

    pub(super) fn get(&self, provider: MediaProvider) -> Option<&SecretString> {
        self.0.get(&provider)
    }
}
