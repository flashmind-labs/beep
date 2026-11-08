//! Context limits from the installed harness's catalog, never a model-name guess.

use super::super::AgentKind;
use beep_proto::node::agent::ModelList;
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};
use tokio::io::AsyncReadExt;

const MAX_CATALOG: u64 = 4 * 1024 * 1024;

#[derive(Deserialize)]
struct Catalog {
    models: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    slug: String,
    context_window: Option<u64>,
    max_context_window: Option<u64>,
    effective_context_window_percent: Option<u64>,
}

pub(super) async fn enrich(kind: &AgentKind, list: &mut ModelList) {
    if kind.name != "codex" {
        return;
    }
    let home = kind
        .env
        .get("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CODEX_HOME").map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")));
    let Some(home) = home else { return };
    let Ok(file) = tokio::fs::File::open(home.join("models_cache.json")).await else {
        return;
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_CATALOG + 1)
        .read_to_end(&mut bytes)
        .await
        .is_err()
        || bytes.len() as u64 > MAX_CATALOG
    {
        return;
    }
    apply(&bytes, list);
}

fn apply(bytes: &[u8], list: &mut ModelList) {
    let Ok(catalog) = serde_json::from_slice::<Catalog>(bytes) else {
        return;
    };
    let models: HashMap<_, _> = catalog
        .models
        .into_iter()
        .map(|m| (m.slug.clone(), m))
        .collect();
    for option in &mut list.models {
        let id = option.id.split('[').next().unwrap_or(&option.id);
        let Some(model) = models.get(id) else {
            continue;
        };
        let Some(percent @ 1..=100) = model.effective_context_window_percent else {
            continue;
        };
        let usable = |window: Option<u64>| {
            window?
                .checked_mul(percent)?
                .checked_div(100)
                .filter(|v| *v > 0)
        };
        option.context_window = usable(model.context_window);
        option.max_context_window = usable(model.max_context_window);
    }
}

#[cfg(test)]
#[path = "../../tests/agents_models_context.rs"]
mod tests;
