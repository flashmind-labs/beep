//! Project persona files: Markdown with YAML frontmatter, as Claude agent
//! files are, plus the Beep keys `engine`, `needs` and `mcp`.

use super::model::WorkerAgent;
use beep_proto::node::Capability;
use flashmind::tools::persona::{Persona, PersonaError};
use serde_yaml::{Mapping, Value};
use std::str::FromStr;

/// A persona and the Beep settings that came with it.
#[derive(Debug)]
pub(crate) struct ProjectPersona {
    pub(crate) persona: Persona,
    /// The coding agent to run the persona on, when the file names one.
    pub(crate) engine: Option<WorkerAgent>,
    /// What a machine must be able to do to run it.
    pub(crate) needs: Vec<Capability>,
    /// MCP servers the persona wants.
    pub(crate) mcp: Vec<String>,
    /// The file the persona was read from.
    pub(crate) file: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PersonaFileError {
    #[error(transparent)]
    Persona(#[from] PersonaError),
    #[error("invalid persona frontmatter: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("unknown engine {0:?}")]
    Engine(String),
    #[error("unknown need {0:?}")]
    Need(String),
}

/// Read the persona in `text`, which came from `file`.
pub(crate) fn parse(file: &str, text: &str) -> Result<ProjectPersona, PersonaFileError> {
    let (mut meta, body) = match split(text) {
        Some(parts) => parts,
        None => {
            // Not a frontmatter file: let the persona parser say why.
            let persona = Persona::from_markdown(text, &[] as &[&str])?;
            return Ok(ProjectPersona {
                persona,
                engine: None,
                needs: Vec::new(),
                mcp: Vec::new(),
                file: file.to_owned(),
            });
        }
    };
    let engine = match meta.remove("engine") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let name = scalar(&value)?;
            Some(WorkerAgent::from_str(&name).map_err(|_| PersonaFileError::Engine(name))?)
        }
    };
    let needs = names(meta.remove("needs"))?
        .into_iter()
        .map(|name| Capability::from_str(&name).map_err(|_| PersonaFileError::Need(name)))
        .collect::<Result<Vec<_>, _>>()?;
    let mcp = names(meta.remove("mcp"))?;
    let rest = format!("---\n{}---\n{body}", serde_yaml::to_string(&meta)?);
    let persona = Persona::from_markdown(&rest, &[] as &[&str])?;
    Ok(ProjectPersona {
        persona,
        engine,
        needs,
        mcp,
        file: file.to_owned(),
    })
}

/// The frontmatter mapping and the text after it, or `None` when `text`
/// has no complete, mapping-shaped frontmatter block.
fn split(text: &str) -> Option<(Mapping, String)> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut yaml = String::new();
    loop {
        let line = lines.next()?;
        if line.trim() == "---" {
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    let Value::Mapping(meta) = serde_yaml::from_str(&yaml).ok()? else {
        return None;
    };
    Some((meta, lines.collect::<Vec<_>>().join("\n")))
}

fn scalar(value: &Value) -> Result<String, serde_yaml::Error> {
    Ok(match value {
        Value::String(text) => text.clone(),
        other => serde_yaml::to_string(other)?.trim().to_owned(),
    })
}

/// A list of names, or a single name.
fn names(value: Option<Value>) -> Result<Vec<String>, serde_yaml::Error> {
    match value {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Sequence(items)) => items.iter().map(scalar).collect(),
        Some(single) => Ok(vec![scalar(&single)?]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persona_reads_beep_keys() {
        let text = "---\nname: blender-rigger\ndescription: Rigs characters\nengine: claude\n\
                    needs: [cuda, blender]\nmcp: [blender]\n---\nYou rig models.\n";
        let found = parse("agents/rigger.md", text).unwrap();
        assert_eq!(found.engine, Some(WorkerAgent::Claude));
        assert_eq!(found.needs, [Capability::Cuda, Capability::Blender]);
        assert_eq!(found.mcp, ["blender"]);
        assert_eq!(found.persona.name, "blender-rigger");
        assert_eq!(found.persona.system_prompt, "You rig models.");
        assert!(found.persona.notices.is_empty());
        assert_eq!(found.file, "agents/rigger.md");
    }

    #[test]
    fn persona_without_beep_keys() {
        let text = "---\nname: reviewer\ndescription: Reviews code\n---\nReview it.\n";
        let found = parse("reviewer.md", text).unwrap();
        assert_eq!(found.engine, None);
        assert!(found.needs.is_empty());
        assert!(found.mcp.is_empty());
    }

    #[test]
    fn persona_unknown_engine_fails() {
        let text = "---\nname: a\ndescription: b\nengine: gemini\n---\nBody\n";
        assert!(matches!(
            parse("a.md", text),
            Err(PersonaFileError::Engine(name)) if name == "gemini"
        ));
    }

    #[test]
    fn persona_unknown_need_fails() {
        let text = "---\nname: a\ndescription: b\nneeds: [quantum]\n---\nBody\n";
        assert!(matches!(
            parse("a.md", text),
            Err(PersonaFileError::Need(name)) if name == "quantum"
        ));
    }

    #[test]
    fn persona_without_frontmatter_fails() {
        assert!(matches!(
            parse("a.md", "Just text"),
            Err(PersonaFileError::Persona(PersonaError::MissingFrontmatter))
        ));
    }
}
