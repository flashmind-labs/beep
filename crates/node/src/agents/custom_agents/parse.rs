//! Reading one custom agent definition: Markdown with YAML frontmatter,
//! a Codex role file, or the agent tables of `opencode.json` and Codex's
//! `config.toml`. Nothing here touches the file system.

use anyhow::{Context, Result, bail};
use serde_json::Value as Json;
use serde_yaml::{Mapping, Value as Yaml};
use toml::{Table, Value as Toml};

/// What one definition says. `error` says what was wrong with it while the
/// rest still shows what could be read.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Found {
    /// `None` when the file names none, so its stem does.
    pub(super) name: Option<String>,
    pub(super) description: String,
    pub(super) model: Option<String>,
    pub(super) tools: Vec<String>,
    pub(super) color: Option<String>,
    pub(super) prompt: String,
    pub(super) error: Option<String>,
}

/// A role `[agents.<name>]` in a Codex `config.toml` declares.
#[derive(Debug, PartialEq)]
pub(super) struct CodexDeclared {
    pub(super) name: String,
    pub(super) description: String,
    /// Its role file, relative to the `config.toml`.
    pub(super) config_file: Option<String>,
}

/// A Markdown definition: YAML frontmatter between `---` lines, then the
/// prompt. Frontmatter that is not valid YAML is read a line at a time for
/// what it can give, and the row says it is broken.
pub(super) fn markdown(text: &str) -> Found {
    let Some(parts) = split_frontmatter(text) else {
        return Found {
            prompt: text.trim().to_owned(),
            error: Some("it has no frontmatter between --- lines".to_owned()),
            ..Found::default()
        };
    };
    let prompt = parts.body.trim().to_owned();
    match serde_yaml::from_str::<Yaml>(parts.yaml) {
        Ok(Yaml::Mapping(meta)) => from_yaml(&meta, prompt),
        Ok(Yaml::Null) => Found {
            prompt,
            ..Found::default()
        },
        Ok(_) => Found {
            prompt,
            error: Some("its frontmatter is not a list of keys".to_owned()),
            ..Found::default()
        },
        Err(e) => Found {
            error: Some(format!("its frontmatter is not valid YAML: {e}")),
            ..from_lines(parts.yaml, prompt)
        },
    }
}

struct Frontmatter<'a> {
    yaml: &'a str,
    body: &'a str,
}

/// The frontmatter and the text after it; `None` without a closed block.
fn split_frontmatter(text: &str) -> Option<Frontmatter<'_>> {
    let text = text.trim_start_matches('\u{feff}');
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some(Frontmatter {
                yaml: &rest[..at],
                body: &rest[at + line.len()..],
            });
        }
        at += line.len();
    }
    None
}

fn from_yaml(meta: &Mapping, prompt: String) -> Found {
    let get = |key: &str| meta.get(key).and_then(yaml_text);
    Found {
        name: get("name"),
        description: get("description").unwrap_or_default(),
        model: get("model"),
        tools: meta.get("tools").map(yaml_tools).unwrap_or_default(),
        color: get("color"),
        prompt,
        error: None,
    }
}

/// A scalar as text; `None` for anything else or an empty string.
fn yaml_text(value: &Yaml) -> Option<String> {
    let text = match value {
        Yaml::String(text) => text.trim().to_owned(),
        Yaml::Number(n) => n.to_string(),
        Yaml::Bool(b) => b.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// `Read, Grep`, a list of names, or OpenCode's map of names to on or off.
fn yaml_tools(value: &Yaml) -> Vec<String> {
    match value {
        Yaml::String(text) => split_names(text),
        Yaml::Sequence(items) => items.iter().filter_map(yaml_text).collect(),
        Yaml::Mapping(map) => map
            .iter()
            .filter_map(|(key, on)| {
                let key = yaml_text(key)?;
                Some(match on {
                    Yaml::Bool(true) => key,
                    Yaml::Bool(false) => format!("{key} off"),
                    other => format!("{key}: {}", yaml_text(other).unwrap_or_default()),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn split_names(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// What `key: value` lines at the start of a line give, for frontmatter
/// YAML refuses, such as a description with an unquoted `: ` in it.
fn from_lines(yaml: &str, prompt: String) -> Found {
    let mut found = Found {
        prompt,
        ..Found::default()
    };
    for line in yaml.lines() {
        if line.starts_with([' ', '\t', '-', '#']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = unquote(value.trim());
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "name" => found.name = Some(value.to_owned()),
            "description" => found.description = value.to_owned(),
            "model" => found.model = Some(value.to_owned()),
            "color" => found.color = Some(value.to_owned()),
            "tools" => found.tools = split_names(value.trim_matches(['[', ']'])),
            _ => {}
        }
    }
    found
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// A Codex role file: `name`, `description` and `developer_instructions`,
/// with `model` among the settings it overrides.
pub(super) fn codex_role(text: &str) -> Result<Found> {
    let table: Table = toml::from_str(text).context("it is not valid TOML")?;
    let get = |key: &str| table.get(key).and_then(toml_text);
    let prompt = get("developer_instructions").unwrap_or_default();
    let error = prompt
        .is_empty()
        .then(|| "it has no developer_instructions, so Codex refuses it".to_owned());
    Ok(Found {
        name: get("name"),
        description: get("description").unwrap_or_default(),
        model: get("model"),
        tools: Vec::new(),
        color: None,
        prompt,
        error,
    })
}

fn toml_text(value: &Toml) -> Option<String> {
    let text = match value {
        Toml::String(text) => text.trim().to_owned(),
        Toml::Integer(n) => n.to_string(),
        Toml::Float(n) => n.to_string(),
        Toml::Boolean(b) => b.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

/// The roles `[agents.<name>]` tables in a Codex `config.toml` declare;
/// the plain settings of `[agents]`, such as `max_threads`, are not roles.
pub(super) fn codex_config(text: &str) -> Result<Vec<CodexDeclared>> {
    let table: Table = toml::from_str(text).context("it is not valid TOML")?;
    let Some(Toml::Table(agents)) = table.get("agents") else {
        return Ok(Vec::new());
    };
    Ok(agents
        .iter()
        .filter_map(|(name, role)| {
            let Toml::Table(role) = role else {
                return None;
            };
            Some(CodexDeclared {
                name: name.clone(),
                description: role
                    .get("description")
                    .and_then(toml_text)
                    .unwrap_or_default(),
                config_file: role.get("config_file").and_then(toml_text),
            })
        })
        .collect())
}

/// The agents in the `agent` object of an `opencode.json` or
/// `opencode.jsonc`, comments and trailing commas allowed.
pub(super) fn opencode_config(text: &str) -> Result<Vec<Found>> {
    let json: Json = serde_json::from_str(&strip_jsonc(text)).context("it is not valid JSON")?;
    let Some(agents) = json.get("agent") else {
        return Ok(Vec::new());
    };
    let Json::Object(agents) = agents else {
        bail!("its agent key is not an object");
    };
    Ok(agents
        .iter()
        .map(|(name, agent)| {
            let get = |key: &str| agent.get(key).and_then(json_text);
            Found {
                name: Some(name.clone()),
                description: get("description").unwrap_or_default(),
                model: get("model"),
                tools: agent.get("tools").map(json_tools).unwrap_or_default(),
                color: get("color"),
                prompt: get("prompt").unwrap_or_default(),
                error: (!agent.is_object()).then(|| format!("{name} is not an object")),
            }
        })
        .collect())
}

fn json_text(value: &Json) -> Option<String> {
    let text = match value {
        Json::String(text) => text.trim().to_owned(),
        Json::Number(n) => n.to_string(),
        Json::Bool(b) => b.to_string(),
        _ => return None,
    };
    (!text.is_empty()).then_some(text)
}

fn json_tools(value: &Json) -> Vec<String> {
    match value {
        Json::Object(map) => map
            .iter()
            .map(|(key, on)| match on {
                Json::Bool(true) => key.clone(),
                Json::Bool(false) => format!("{key} off"),
                other => format!("{key}: {other}"),
            })
            .collect(),
        Json::Array(items) => items.iter().filter_map(json_text).collect(),
        Json::String(text) => split_names(text),
        _ => Vec::new(),
    }
}

/// JSONC as plain JSON: `//` and `/* */` comments dropped, and commas
/// before a closing `}` or `]`. Strings are left as they are.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(s) = chars.next() {
                    out.push(s);
                    match s {
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                out.push(escaped);
                            }
                        }
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for s in chars.by_ref() {
                    if s == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for s in chars.by_ref() {
                    if last == '*' && s == '/' {
                        break;
                    }
                    last = s;
                }
            }
            '}' | ']' => {
                let kept = out.trim_end().len();
                if out[..kept].ends_with(',') {
                    out.truncate(kept - 1);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broken_yaml_still_gives_what_its_lines_say() {
        let text = "---\nname: helper\ndescription: Use it when: tests fail\n  bad: [\n---\nHelp.";
        let found = markdown(text);
        assert_eq!(found.name.as_deref(), Some("helper"));
        assert_eq!(found.description, "Use it when: tests fail");
        assert!(found.error.unwrap().contains("not valid YAML"));
    }

    #[test]
    fn codex_role_file() {
        let text = "name = \"explorer\"\ndescription = \"Reads code\"\nmodel = \"gpt-5\"\n\
                    developer_instructions = \"\"\"\nRead, never write.\n\"\"\"\n";
        let found = codex_role(text).unwrap();
        assert_eq!(found.name.as_deref(), Some("explorer"));
        assert_eq!(found.model.as_deref(), Some("gpt-5"));
        assert_eq!(found.prompt, "Read, never write.");
        assert_eq!(found.error, None);
        assert!(codex_role("name = \"x\"").unwrap().error.is_some());
        assert!(codex_role("name = ").is_err());
    }

    #[test]
    fn codex_config_roles_skip_plain_settings() {
        let text = "model = \"gpt-5\"\n[agents]\nmax_threads = 4\n\
                    [agents.reviewer]\ndescription = \"Reviews\"\nconfig_file = \"agents/reviewer.toml\"\n";
        assert_eq!(
            codex_config(text).unwrap(),
            [CodexDeclared {
                name: "reviewer".to_owned(),
                description: "Reviews".to_owned(),
                config_file: Some("agents/reviewer.toml".to_owned()),
            }]
        );
    }

    #[test]
    fn opencode_jsonc_with_comments_and_trailing_commas() {
        let text = r#"{
            // the agents
            "$schema": "https://example.com/config.json",
            "agent": {
                "planner": {
                    "description": "Plans // not a comment",
                    "model": "provider/model-a", /* inline */
                    "prompt": "{file:./prompts/plan.txt}",
                    "tools": { "write": false, },
                },
            },
        }"#;
        let found = opencode_config(text).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name.as_deref(), Some("planner"));
        assert_eq!(found[0].description, "Plans // not a comment");
        assert_eq!(found[0].tools, ["write off"]);
        assert_eq!(found[0].prompt, "{file:./prompts/plan.txt}");
        assert!(opencode_config("{\"model\": \"x\"}").unwrap().is_empty());
        assert!(opencode_config("{").is_err());
    }
}
