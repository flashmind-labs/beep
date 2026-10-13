//! App tool results that point at a file, such as a OneDrive download or a
//! mail attachment, saved into the user's folder as they arrive. Composio
//! hands back a short-lived `s3url` rather than the file, and code in the
//! sandbox has no network to fetch it, so the model would otherwise have to
//! know to call `download_file` first.

use crate::{sandbox::Sandbox, user::User};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::{Map, Value};
use std::sync::Arc;

/// Extra time a call gets for saving its files.
const SAVE_SECS: u64 = 120;

/// Wraps each tool so the files its successful results link to are saved.
pub(crate) fn wrap(
    tools: Vec<Arc<dyn Tool>>,
    sandbox: &Arc<Sandbox>,
    user: &Arc<User>,
) -> Vec<Arc<dyn Tool>> {
    tools
        .into_iter()
        .map(|tool| {
            Arc::new(SaveFiles {
                tool,
                sandbox: sandbox.clone(),
                user: user.clone(),
            }) as Arc<dyn Tool>
        })
        .collect()
}

struct SaveFiles {
    tool: Arc<dyn Tool>,
    sandbox: Arc<Sandbox>,
    user: Arc<User>,
}

/// A file a result links to.
struct Link {
    url: String,
    name: Option<String>,
}

#[async_trait]
impl Tool for SaveFiles {
    fn name(&self) -> &str {
        self.tool.name()
    }

    fn description(&self) -> &str {
        self.tool.description()
    }

    fn parameters(&self) -> Value {
        self.tool.parameters()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let mut result = self.tool.execute(ctx).await?;
        let ToolResult::Success { output, .. } = &mut result else {
            return Ok(result);
        };
        let Ok(mut value) = serde_json::from_str::<Value>(output) else {
            return Ok(result);
        };
        let mut links = Vec::new();
        find_links(&value, &mut links);
        if links.is_empty() {
            return Ok(result);
        }
        let mut saved = Vec::with_capacity(links.len());
        for link in links {
            let outcome = self
                .sandbox
                .download(self.user.external_id(), &link.url, link.name.as_deref())
                .await;
            if let Err(e) = &outcome {
                log::warn!("files: unable to save {}: {e:#}", self.tool.name());
            }
            saved.push(outcome.map(|saved| saved.path));
        }
        mark_saved(&mut value, &mut saved.into_iter());
        *output = value.to_string();
        Ok(result)
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.tool.timeout_secs().map(|secs| secs + SAVE_SECS)
    }

    fn max_output_bytes(&self) -> usize {
        self.tool.max_output_bytes()
    }

    fn max_output_lines(&self) -> usize {
        self.tool.max_output_lines()
    }

    fn humanize(&self, args: &Value) -> String {
        self.tool.humanize(args)
    }

    fn concurrent(&self) -> bool {
        self.tool.concurrent()
    }
}

/// The `s3url` of every object under `value`, in walk order.
fn find_links(value: &Value, links: &mut Vec<Link>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| find_links(item, links)),
        Value::Object(fields) => {
            if let Some(url) = s3url(fields) {
                links.push(Link {
                    url: url.to_owned(),
                    name: fields
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
            }
            fields.values().for_each(|field| find_links(field, links));
        }
        _ => {}
    }
}

/// Adds where each linked file went, walking in the same order as
/// `find_links`: `saved_as` with its name in the user's folder, or
/// `save_error`.
fn mark_saved(value: &mut Value, saved: &mut impl Iterator<Item = anyhow::Result<String>>) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(|item| mark_saved(item, saved)),
        Value::Object(fields) => {
            let outcome = s3url(fields).and_then(|_| saved.next());
            fields
                .values_mut()
                .for_each(|field| mark_saved(field, saved));
            match outcome {
                Some(Ok(path)) => {
                    fields.insert("saved_as".into(), path.into());
                }
                Some(Err(e)) => {
                    fields.insert("save_error".into(), format!("{e:#}").into());
                }
                None => {}
            }
        }
        _ => {}
    }
}

fn s3url(fields: &Map<String, Value>) -> Option<&str> {
    fields.get("s3url").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use serde_json::json;

    #[test]
    fn marks_each_link_where_it_was_found() {
        let mut value = json!({
            "content": {"name": "contract.pdf", "s3url": "https://example.com/a"},
            "attachments": [
                {"name": "one.pdf", "s3url": "https://example.com/b"},
                {"name": "plain"}
            ]
        });
        let mut links = Vec::new();
        find_links(&value, &mut links);
        let saved = links.iter().map(|link| match link.name.as_deref() {
            Some("one.pdf") => Ok("one.pdf".to_owned()),
            _ => Err(anyhow!("too big")),
        });
        let saved: Vec<_> = saved.collect();
        mark_saved(&mut value, &mut saved.into_iter());
        assert_eq!(value["attachments"][0]["saved_as"], "one.pdf");
        assert_eq!(value["content"]["save_error"], "too big");
        assert!(value["attachments"][1].get("saved_as").is_none());
    }
}
