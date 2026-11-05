//! What the local model picker has beyond the chip's `Models`: the catalog with its features,
//! context and prices, and the preferred upstream provider. These are this Mac's own commands, so
//! the server's replies keep their shape.

use super::local::{NO_KEY, bad, chat_dir, key};
use crate::{
    beep::Service,
    error::{DesktopError, Result},
    fleet::projects::FleetState,
};
use beep_assistant::{
    Endpoint, Feature, ModelInfo, OPENROUTER_BASE, Provider, ProviderPref, http_client,
    list_models, list_providers, load_prefs, save_prefs,
};
use beep_proto::ApiProvider;
use serde::Serialize;
use tauri::AppHandle;

/// The models with a feature and the provider saved with the model it was picked for.
#[derive(Serialize)]
pub struct Picker {
    models: Vec<ModelInfo>,
    provider: Option<ProviderPref>,
}

async fn endpoint(fleet: &FleetState<'_>) -> Result<Endpoint> {
    let key = key(fleet, ApiProvider::OpenRouter)
        .await
        .ok_or_else(|| DesktopError::BadRequest(NO_KEY.into()))?;
    Ok(Endpoint {
        base: OPENROUTER_BASE.to_owned(),
        key,
    })
}

/// The catalog for the local chat's picker, newest first: models with `feature`, or all of them
/// with none. The chat picker asks for chat models.
#[tauri::command]
pub async fn local_picker(
    app: AppHandle,
    fleet: FleetState<'_>,
    feature: Option<Feature>,
) -> Result<Picker> {
    let endpoint = endpoint(&fleet).await?;
    let models = list_models(&endpoint, feature).await.map_err(bad)?;
    let prefs = load_prefs(&chat_dir(&app)?).await;
    Ok(Picker {
        models,
        provider: prefs.provider,
    })
}

/// The upstream providers OpenRouter routes `model` to. Asked for only when the user opens them.
#[tauri::command]
pub async fn local_providers(
    beep: Service<'_>,
    fleet: FleetState<'_>,
    model: String,
) -> Result<Vec<Provider>> {
    if beep.server().is_some() {
        return Err(DesktopError::BadRequest("Not a local chat.".into()));
    }
    let endpoint = endpoint(&fleet).await?;
    list_providers(&http_client(), &endpoint, &model)
        .await
        .map_err(bad)
}

/// Saves the preferred provider of `model`, or clears it with no tag.
#[tauri::command]
pub async fn set_local_provider(
    app: AppHandle,
    model: String,
    tag: Option<String>,
) -> Result<Option<ProviderPref>> {
    let dir = chat_dir(&app)?;
    let mut prefs = load_prefs(&dir).await;
    prefs.provider = tag.map(|tag| ProviderPref { model, tag });
    save_prefs(&dir, &prefs).await.map_err(bad)?;
    Ok(prefs.provider)
}
