use std::path::{Path, PathBuf};

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{Ctx, Error as JsError, Module};

/// Source files one plugin may load, the entry included.
const MAX_FILES: usize = 32;
/// Source bytes one plugin may load, over all its files.
const MAX_SOURCE_BYTES: usize = 512 * 1024;

/// Turns an import into a JavaScript path inside the plugin folder, never
/// above it. `base` is the importing file's name.
pub(crate) struct Imports;

impl Resolver for Imports {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let refuse = |why: &str| JsError::new_resolving_message(base, name, why.to_owned());
        let relative = name.starts_with("./") || name.starts_with("../");
        if !relative || name.contains('\\') {
            return Err(refuse(
                "only relative imports such as \"./draw.js\" are allowed",
            ));
        }

        let mut parts: Vec<&str> = base
            .rsplit_once('/')
            .map_or("", |(folder, _)| folder)
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        for part in name.split('/') {
            match part {
                "" | "." => {}
                ".." if parts.pop().is_none() => {
                    return Err(refuse("the import leaves the plugin folder"));
                }
                ".." => {}
                other => parts.push(other),
            }
        }

        let resolved = parts.join("/");
        match Path::new(&resolved)
            .extension()
            .and_then(|ext| ext.to_str())
        {
            Some("js") => Ok(resolved),
            Some("ts" | "tsx") => Err(refuse(
                "build TypeScript to JavaScript before importing it; only .js files can be imported",
            )),
            _ => Err(refuse("only .js files can be imported")),
        }
    }
}

/// Reads compiled JavaScript modules and counts them against the plugin's limits.
pub(crate) struct Sources {
    root: PathBuf,
    files: usize,
    bytes: usize,
}

impl Sources {
    /// `root` is the plugin folder, canonical.
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            files: 0,
            bytes: 0,
        }
    }

    fn read(&mut self, name: &str) -> Result<String, String> {
        if Path::new(name).extension().and_then(|ext| ext.to_str()) != Some("js") {
            return Err("build plugin sources to JavaScript before loading them; only .js files can be loaded".to_owned());
        }

        let path = self.root.join(name);
        let real = path
            .canonicalize()
            .map_err(|error| format!("cannot read {name}: {error}"))?;
        if !real.starts_with(&self.root) {
            return Err(format!("{name} is outside the plugin folder"));
        }

        let text = std::fs::read_to_string(&real)
            .map_err(|error| format!("cannot read {name}: {error}"))?;
        self.files += 1;
        self.bytes = self.bytes.saturating_add(text.len());
        if self.files > MAX_FILES || self.bytes > MAX_SOURCE_BYTES {
            return Err(format!(
                "the plugin loads more than {MAX_FILES} files or {} KB of source",
                MAX_SOURCE_BYTES / 1024
            ));
        }

        Ok(text)
    }
}

impl Loader for Sources {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        let script = self
            .read(name)
            .map_err(|why| JsError::new_loading_message(name, why))?;

        Module::declare(ctx.clone(), name, script)
    }
}

#[cfg(test)]
#[path = "tests/source.rs"]
mod tests;
