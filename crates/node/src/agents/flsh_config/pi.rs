//! The user's OpenRouter routing, written into pi's `models.json` before pi
//! starts (`apply`): the routing for all OpenRouter models in
//! `providers.openrouter.compat.openRouterRouting`, and a model's own
//! quantizations in `providers.openrouter.modelOverrides."<model>"`, which pi
//! lays over the first key by key. Compaction stays flsh's: pi sizes its own
//! by the tokens it keeps free, not by a threshold.

use super::{RoutingKey, WRITING, read_capped, read_owned, write_atomic};
use crate::lock;
use anyhow::{Context, Result, bail};
use beep_proto::{FlshQuantization, FlshSettings};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeSet, fs, path::Path};
use strum::VariantArray;

/// pi's models file in its agent folder.
const MODELS: &str = "models.json";
/// What Beep set in it last time, next to it.
const OWNED: &str = "beep-owned.json";
/// The provider pi routes OpenRouter models through.
const OPENROUTER: [&str; 2] = ["providers", "openrouter"];
/// Where pi reads routing in a provider or a model override.
const ROUTING: [&str; 2] = ["compat", "openRouterRouting"];

/// What Beep set in pi's models file, as `beep-owned.json` keeps it.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Owned {
    routing: BTreeSet<RoutingKey>,
    /// The `modelOverrides` keys whose quantizations Beep set.
    quantizations: BTreeSet<String>,
}

/// Writes the routing in `settings` into the `models.json` in pi's agent
/// folder `dir`, making it when there is none. Blocking. A file with
/// comments, which pi allows, is not plain JSON and is left alone with an
/// error.
pub(in crate::agents) fn apply(dir: &Path, settings: &FlshSettings) -> Result<()> {
    let _writing = lock(&WRITING);
    let path = dir.join(MODELS);
    let text = read_capped(&path)?;
    let mut doc: Value = match text.trim() {
        "" => Value::Object(Map::new()),
        text => serde_json::from_str(text).with_context(|| {
            format!(
                "{} is not plain JSON (comments?), so Beep cannot write its routing",
                path.display()
            )
        })?,
    };
    let read = doc.clone();
    let owned_path = dir.join(OWNED);
    let before = read_owned(&owned_path);
    let owned = edit(&mut doc, settings, &before)?;

    if doc != read {
        let mut edited = serde_json::to_string_pretty(&doc)?;
        edited.push('\n');
        fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        write_atomic(&path, edited.as_bytes())?;
        log::info!("pi routing written; path={}", path.display());
    }

    if owned != before {
        write_atomic(&owned_path, &serde_json::to_vec(&owned)?)?;
    }

    Ok(())
}

/// `doc` with the routing in `settings` set, and Beep's keys from `before`
/// that `settings` no longer sets removed. Gives the keys Beep owns now.
fn edit(doc: &mut Value, settings: &FlshSettings, before: &Owned) -> Result<Owned> {
    let mut owned = Owned::default();
    let provider = [OPENROUTER.as_slice(), &ROUTING].concat();

    let quantizations =
        (!settings.quantizations.is_empty()).then(|| names(&settings.quantizations));
    let routing = [
        (RoutingKey::Zdr, settings.zdr.map(Value::Bool)),
        (RoutingKey::Quantizations, quantizations),
        (
            RoutingKey::Sort,
            settings.sort.map(|sort| Value::from(<&str>::from(sort))),
        ),
        (
            RoutingKey::AllowFallbacks,
            settings.allow_fallbacks.map(Value::Bool),
        ),
    ];

    for (key, item) in routing {
        match item {
            Some(item) => {
                object(doc, &provider)?.insert(key.as_ref().to_owned(), item);
                owned.routing.insert(key);
            }
            None if before.routing.contains(&key) => remove(doc, &provider, key.as_ref()),
            None => {}
        }
    }

    for rule in &settings.models {
        let Some(quantizations) = &rule.quantizations else {
            continue;
        };
        // pi passes the list to OpenRouter as it is, so any precision is
        // every one OpenRouter names.
        let allowed = match quantizations.as_slice() {
            [] => FlshQuantization::VARIANTS,
            some => some,
        };
        let model = rule.model.capability_name();
        object(doc, &overridden(model))?.insert("quantizations".to_owned(), names(allowed));
        owned.quantizations.insert(model.to_owned());
    }

    for model in before.quantizations.difference(&owned.quantizations) {
        remove(doc, &overridden(model), "quantizations");
    }

    Ok(owned)
}

/// The path of `model`'s own routing.
fn overridden(model: &str) -> Vec<&str> {
    [OPENROUTER.as_slice(), &["modelOverrides", model], &ROUTING].concat()
}

/// Quantizations as OpenRouter spells them.
fn names(quantizations: &[FlshQuantization]) -> Value {
    quantizations
        .iter()
        .map(|q| Value::from(<&str>::from(*q)))
        .collect()
}

/// The object at `path` in `doc`, made when missing.
fn object<'a>(doc: &'a mut Value, path: &[&str]) -> Result<&'a mut Map<String, Value>> {
    let mut at = doc;

    for key in path {
        let Value::Object(map) = at else {
            bail!("`{key}` in pi's models.json is not inside an object");
        };
        at = map.entry(*key).or_insert_with(|| Value::Object(Map::new()));
    }

    match at {
        Value::Object(map) => Ok(map),
        _ => bail!("`{}` in pi's models.json is not an object", path.join(".")),
    }
}

/// Removes `key` from the object at `path`, then each object on the way
/// that it leaves empty: pi refuses a provider with nothing in it.
fn remove(doc: &mut Value, path: &[&str], key: &str) {
    let Some(Value::Object(map)) = path.iter().try_fold(&mut *doc, |at, key| at.get_mut(*key))
    else {
        return;
    };
    map.remove(key);

    for depth in (1..=path.len()).rev() {
        let (last, parent) = (path[depth - 1], &path[..depth - 1]);
        let Some(Value::Object(map)) = parent
            .iter()
            .try_fold(&mut *doc, |at, key| at.get_mut(*key))
        else {
            return;
        };
        let empty = map
            .get(last)
            .is_some_and(|v| v.as_object().is_some_and(Map::is_empty));

        if !empty {
            return;
        }

        map.remove(last);
    }
}

#[cfg(test)]
#[path = "../../tests/agents_pi_config.rs"]
mod tests;
