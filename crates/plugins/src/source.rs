use std::path::Path;

use oxc::allocator::Allocator;
use oxc::codegen::Codegen;
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};
use thiserror::Error;

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
