//! The Assistant page's Dictation section: which provider and model transcribe what Option held
//! down records. The pick is a small file on this Mac, and only a provider with a saved key can
//! be picked. The keys themselves never leave Rust.

use crate::{
    beep::identity::data_dir,
    error::{DesktopError, Result},
    fleet::projects::{FleetState, desk},
    voice::{Voice, api_provider},
};
use beep_dictation::{ModelChoice, Provider, Settings, openrouter_models, save_settings};
use serde::Serialize;
use std::sync::Arc;
use strum::IntoEnumIterator;
use tauri::{AppHandle, State};

/// A provider with a saved key, and the model used until another is typed.
#[derive(Serialize)]
pub(crate) struct Offered {
    provider: Provider,
    name: &'static str,
    default_model: &'static str,
}

#[derive(Serialize)]
pub(crate) struct DictationView {
    provider: Option<Provider>,
    model: String,
    offered: Vec<Offered>,
}

fn bad(e: impl ToString) -> DesktopError {
    DesktopError::BadRequest(e.to_string())
}

/// The pick, and the providers it can be changed to.
#[tauri::command]
pub(crate) async fn dictation_get(
    voice: State<'_, Arc<Voice>>,
    fleet: FleetState<'_>,
) -> Result<DictationView> {
    let desk = desk(&fleet).await?;
    let settings = voice.dictation_settings();
    let offered = Provider::iter()
        .filter(|p| desk.api_key(api_provider(*p)).is_some())
        .map(|provider| Offered {
            provider,
            name: provider.name(),
            default_model: provider.default_model(),
        })
        .collect();
    Ok(DictationView {
        provider: settings.provider,
        model: settings.model,
        offered,
    })
}

/// Saves the pick. No provider turns dictation off; a typed model is trimmed, and empty means the
/// provider's own.
#[tauri::command]
pub(crate) async fn dictation_set(
    app: AppHandle,
    voice: State<'_, Arc<Voice>>,
    fleet: FleetState<'_>,
    provider: Option<Provider>,
    model: String,
) -> Result<()> {
    if let Some(provider) = provider
        && desk(&fleet)
            .await?
            .api_key(api_provider(provider))
            .is_none()
    {
        return Err(bad(format!("Add a {} key first.", provider.name())));
    }
    let settings = Settings {
        provider,
        model: model.trim().to_owned(),
    };
    save_settings(&data_dir(&app)?, &settings)
        .await
        .map_err(bad)?;
    voice.set_dictation_settings(settings);
    Ok(())
}

/// OpenRouter's transcription models, by name.
#[tauri::command]
pub(crate) async fn dictation_models(fleet: FleetState<'_>) -> Result<Vec<ModelChoice>> {
    let key = desk(&fleet)
        .await?
        .api_key(api_provider(Provider::OpenRouter))
        .ok_or_else(|| bad("Add an OpenRouter key first."))?;
    openrouter_models(&key).await.map_err(bad)
}
