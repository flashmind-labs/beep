//! The user's flsh settings, written into flsh's own `config.toml` before
//! flsh starts (`apply`). flsh reads routing and compaction from that file
//! only: it has no flag, variable or second config file for them.

use crate::lock;
use anyhow::{Context, Result, bail};
use beep_proto::{FlshAutoCompact, FlshModel, FlshSettings};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fmt::Display,
    fs,
    io::{self, Read, Write},
    path::Path,
    sync::Mutex,
};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, value};

/// flsh's config file in its config folder.
const CONFIG: &str = "config.toml";
/// What Beep set in it last time, next to it, so a setting cleared in Beep
/// goes back to flsh's default without touching a key the user set by hand.
const OWNED: &str = "beep-owned.json";
/// The largest config read; flsh's own is a few kilobytes.
const MAX_CONFIG: u64 = 1024 * 1024;

/// One write at a time, so two flsh sessions starting together do not
/// lose each other's record of what Beep owns.
static WRITING: Mutex<()> = Mutex::new(());

/// The keys of `[openrouter.routing]` Beep sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RoutingKey {
    Zdr,
    Quantizations,
    Sort,
    AllowFallbacks,
}

impl RoutingKey {
    /// Its key in flsh's config.
    fn as_str(self) -> &'static str {
        match self {
            Self::Zdr => "zdr",
            Self::Quantizations => "quantizations",
            Self::Sort => "sort",
            Self::AllowFallbacks => "allow_fallbacks",
        }
    }
}

/// What Beep set in flsh's config, as `beep-owned.json` keeps it.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Owned {
    routing: BTreeSet<RoutingKey>,
    autocompact: bool,
    /// The `auto_compact_models` keys, as Beep wrote them.
    models: BTreeSet<String>,
}

/// Writes `settings` into the `config.toml` in flsh's config folder `dir`,
/// making it when there is none. Blocking.
pub(super) fn apply(dir: &Path, settings: &FlshSettings) -> Result<()> {
    let _writing = lock(&WRITING);
    let path = dir.join(CONFIG);
    let text = read_capped(&path)?;
    let mut doc: DocumentMut = text
        .parse()
        .with_context(|| format!("{} is not valid TOML", path.display()))?;
    let owned_path = dir.join(OWNED);
    let before = read_owned(&owned_path);
    let owned = edit(&mut doc, settings, &before)?;
    let edited = doc.to_string();
    if edited != text {
        fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        write_atomic(&path, edited.as_bytes())?;
        log::info!("flsh settings written; path={}", path.display());
    }
    if owned != before {
        write_atomic(&owned_path, &serde_json::to_vec(&owned)?)?;
    }
    Ok(())
}

/// `doc` with `settings` set, and Beep's keys from `before` that
/// `settings` no longer sets removed. Gives the keys Beep owns now. Every
/// other key, comment and blank line stays as it was.
fn edit(doc: &mut DocumentMut, settings: &FlshSettings, before: &Owned) -> Result<Owned> {
    let root = doc.as_table_mut();
    let mut owned = Owned::default();

    let quantizations = (!settings.quantizations.is_empty()).then(|| {
        let names = settings.quantizations.iter().map(<&str>::from);
        value(names.collect::<Array>())
    });
    let routing = [
        (RoutingKey::Zdr, settings.zdr.map(value)),
        (RoutingKey::Quantizations, quantizations),
        (
            RoutingKey::Sort,
            settings.sort.map(|sort| value(<&str>::from(sort))),
        ),
        (
            RoutingKey::AllowFallbacks,
            settings.allow_fallbacks.map(value),
        ),
    ];
    for (key, item) in routing {
        let name = key.as_str();
        match item {
            Some(item) => {
                let openrouter = table(root, "openrouter")?;
                table(openrouter, "routing")?.insert(name, item);
                owned.routing.insert(key);
            }
            None if before.routing.contains(&key) => {
                if let Some(routing) = existing(root, &["openrouter", "routing"]) {
                    routing.remove(name);
                }
            }
            None => {}
        }
    }

    match &settings.autocompact {
        // flsh reads `enable`, or `enabled` as another name for it; both
        // at once is an error.
        Some(rule) => {
            set_rule(table(root, "autocompact")?, rule, "enable", "enabled");
            owned.autocompact = true;
        }
        None if before.autocompact => {
            root.remove("autocompact");
        }
        None => {}
    }

    for rule in &settings.models {
        let key = rule.model.to_string();
        let models = table(root, "auto_compact_models")?;
        // The same model spelled another way would be a second rule for it.
        let others: Vec<String> = models
            .iter()
            .filter(|(name, _)| *name != key && same_model(name, &rule.model))
            .map(|(name, _)| name.to_owned())
            .collect();
        for name in others {
            models.remove(&name);
        }
        // Per model flsh writes `enabled`, and reads `enable` too.
        set_rule(table(models, &key)?, &rule.autocompact, "enabled", "enable");
        owned.models.insert(key);
    }
    let dropped: Vec<&String> = before.models.difference(&owned.models).collect();
    if !dropped.is_empty()
        && let Some(models) = existing(root, &["auto_compact_models"])
    {
        for key in dropped {
            models.remove(key);
        }
    }
    Ok(owned)
}

/// Sets `rule` in `table` under the key `name`, dropping its other name
/// `alias`.
fn set_rule(table: &mut dyn TableLike, rule: &FlshAutoCompact, name: &str, alias: &str) {
    table.remove(alias);
    table.insert(name, value(rule.enabled));
    match rule.tokens {
        Some(tokens) => {
            table.insert("tokens", value(i64::from(tokens)));
        }
        None => {
            table.remove("tokens");
        }
    }
}

/// Whether the config key `name` names `model`.
fn same_model(name: &str, model: &FlshModel) -> bool {
    name.parse::<FlshModel>()
        .is_ok_and(|parsed| &parsed == model)
}

/// The table `key` in `parent`, made when missing. A table made here has
/// no header of its own until it has keys.
fn table<'a>(parent: &'a mut dyn TableLike, key: &str) -> Result<&'a mut dyn TableLike> {
    if !parent.contains_key(key) {
        let mut table = Table::new();
        table.set_implicit(true);
        parent.insert(key, Item::Table(table));
    }
    match parent.get_mut(key).and_then(Item::as_table_like_mut) {
        Some(table) => Ok(table),
        None => bail!("`{key}` in flsh's config is not a table"),
    }
}

/// The table at `path` under `root`, when there is one.
fn existing<'a>(root: &'a mut dyn TableLike, path: &[&str]) -> Option<&'a mut dyn TableLike> {
    let mut at = root;
    for key in path {
        at = at.get_mut(key)?.as_table_like_mut()?;
    }
    Some(at)
}

/// The file at `path`, empty when there is none.
fn read_capped(path: &Path) -> Result<String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
    };
    let mut text = String::new();
    file.take(MAX_CONFIG + 1)
        .read_to_string(&mut text)
        .with_context(|| format!("reading {}", path.display()))?;
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > MAX_CONFIG {
        bail!("{} is larger than {MAX_CONFIG} bytes", path.display());
    }
    Ok(text)
}

/// What Beep set last time. A record that does not read counts as none:
/// Beep then leaves every key it does not set now as it is.
fn read_owned(path: &Path) -> Owned {
    let ignored = |e: &dyn Display| {
        log::warn!(
            "flsh settings record not read; path={} error={e}",
            path.display()
        );
        Owned::default()
    };
    let text = match fs::read(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Owned::default(),
        Err(e) => return ignored(&e),
    };
    serde_json::from_slice(&text).unwrap_or_else(|e| ignored(&e))
}

/// Replaces `path` with `bytes` in one rename, so flsh never reads half a
/// file, keeping the permissions it had (flsh's config holds API keys).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("a config file has a folder")?;
    let name = path
        .file_name()
        .context("a config file has a name")?
        .to_string_lossy();
    let temp = dir.join(format!(".{name}.beep-{}.tmp", std::process::id()));
    let written = (|| -> io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        match fs::metadata(path) {
            Ok(meta) => file.set_permissions(meta.permissions())?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => private(&file)?,
            Err(e) => return Err(e),
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if let Err(e) = written {
        // The temporary file is ours; a failed removal leaves only it.
        if let Err(removed) = fs::remove_file(&temp)
            && removed.kind() != io::ErrorKind::NotFound
        {
            log::debug!(
                "temporary file kept; path={} error={removed}",
                temp.display()
            );
        }
        return Err(e).with_context(|| format!("writing {}", path.display()));
    }
    Ok(())
}

/// Only the owner may read a new file.
#[cfg(unix)]
fn private(file: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn private(_: &fs::File) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
