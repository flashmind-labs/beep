use std::path::{Path, PathBuf};

use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{Ctx, Error as JsError, Module};
use thiserror::Error;

/// Source files one plugin may load, the entry included.
const MAX_FILES: usize = 32;
/// Source bytes one plugin may load, over all its files.
const MAX_SOURCE_BYTES: usize = 512 * 1024;

#[derive(Debug, Error)]
pub(crate) enum SourceError {
    #[error("unsupported source file {0}")]
    SourceType(String),
    #[error("{0}")]
    Syntax(String),
}

/// Strips TypeScript types, returning JavaScript. `path` picks the dialect by extension.
pub(crate) fn strip_types(source: &str, path: &Path) -> Result<String, SourceError> {
    let source_type = SourceType::from_path(path)
        .map_err(|_| SourceError::SourceType(path.display().to_string()))?;
    let allocator = Allocator::default();

    let parsed = Parser::new(&allocator, source, source_type).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(SourceError::Syntax(join(&parsed.diagnostics)));
    }
    let mut program = parsed.program;

    let semantic = SemanticBuilder::new()
        .with_excess_capacity(2.0)
        .build(&program);
    let scoping = semantic.semantic.into_scoping();

    let transformed = Transformer::new(&allocator, path, &TransformOptions::default())
        .build_with_scoping(scoping, &mut program);
    if !transformed.diagnostics.is_empty() {
        return Err(SourceError::Syntax(join(&transformed.diagnostics)));
    }

    Ok(Codegen::new().build(&program).code)
}

/// Turns an import into a path inside the plugin folder: relative, `.ts` or
/// `.js`, never above the folder. `base` is the importing file's name.
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
                "only relative imports such as \"./draw.ts\" are allowed",
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
            Some("ts" | "js") => Ok(resolved),
            _ => Err(refuse("only .ts and .js files can be imported")),
        }
    }
}

/// Reads the modules a plugin imports, with their types stripped, and counts
/// what it loaded against the plugin's limits.
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

        strip_types(&text, Path::new(name)).map_err(|error| error.to_string())
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

fn join(errors: &[oxc::diagnostics::OxcDiagnostic]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
#[path = "tests/source.rs"]
mod tests;
