//! What the tool schemas of one model request cost.

use crate::apps::{CATALOG, custom_mcp};
use flashmind::types::{ToolDefinition, ToolRegistry};
use std::fmt::{self, Display};

/// About this many bytes of schema make one token.
const BYTES_PER_TOKEN: usize = 4;

/// Where a tool comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    BuiltIn,
    Composio,
    Fastmail,
    /// A server the user added by URL.
    AddedMcp,
}

impl Source {
    /// The source of tool `name`, matched in upper case.
    pub fn of(name: &str) -> Self {
        let slug = name.to_ascii_uppercase();
        if custom_mcp::is_custom(name) {
            Source::AddedMcp
        } else if slug.starts_with("FASTMAIL_") {
            Source::Fastmail
        } else if CATALOG
            .iter()
            .any(|app| app.tools.iter().any(|tool| tool.slug == slug))
        {
            Source::Composio
        } else {
            Source::BuiltIn
        }
    }
}

/// A number of tools and the bytes their definitions take.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cost {
    pub tools: usize,
    pub bytes: usize,
}

impl Cost {
    fn add(&mut self, bytes: usize) {
        self.tools += 1;
        self.bytes += bytes;
    }

    /// An estimate of the tokens the definitions take.
    pub fn tokens(&self) -> usize {
        self.bytes.div_ceil(BYTES_PER_TOKEN)
    }
}

impl Display for Cost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} tools {} B ~{} tok",
            self.tools,
            self.bytes,
            self.tokens()
        )
    }
}

/// The bytes one definition takes: its name, description and schema.
pub fn size(definition: &ToolDefinition) -> usize {
    definition.name.len() + definition.description.len() + definition.parameters.to_string().len()
}

/// The cost of a request's tools, split by where they come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Costs {
    pub built_in: Cost,
    pub composio: Cost,
    pub fastmail: Cost,
    pub added_mcp: Cost,
}

impl Costs {
    pub fn of(definitions: &[ToolDefinition]) -> Self {
        let mut costs = Self::default();
        for definition in definitions {
            let bytes = size(definition);
            match Source::of(&definition.name) {
                Source::BuiltIn => costs.built_in.add(bytes),
                Source::Composio => costs.composio.add(bytes),
                Source::Fastmail => costs.fastmail.add(bytes),
                Source::AddedMcp => costs.added_mcp.add(bytes),
            }
        }
        costs
    }

    /// Every connected app's tools together.
    pub fn apps(&self) -> Cost {
        Cost {
            tools: self.composio.tools + self.fastmail.tools + self.added_mcp.tools,
            bytes: self.composio.bytes + self.fastmail.bytes + self.added_mcp.bytes,
        }
    }

    pub fn total(&self) -> Cost {
        let apps = self.apps();
        Cost {
            tools: self.built_in.tools + apps.tools,
            bytes: self.built_in.bytes + apps.bytes,
        }
    }
}

impl Display for Costs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "total {}; built-in {}; apps {} (composio {}, fastmail {}, added mcp {})",
            self.total(),
            self.built_in,
            self.apps(),
            self.composio,
            self.fastmail,
            self.added_mcp
        )
    }
}

/// Logs at debug what `registry`'s tools add to each request of a `kind` run.
pub(crate) fn log(kind: &str, registry: &ToolRegistry) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let costs = Costs::of(&registry.definitions());
    log::debug!("tool cost: kind={kind} {costs}");
}
