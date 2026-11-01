//! App tool results with HTML in them, such as an email body, turned into
//! Markdown before the model reads them. A newsletter's markup and CSS can be
//! ten times the length of its text, and the line that matters gets lost in it.

use crate::paging;
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use std::sync::Arc;

/// Line width for the Markdown, wide enough that sentences stay on one line.
const WIDTH: usize = 200;

/// Tags that mark a string as HTML rather than text that happens to hold a `<`.
const HTML_TAGS: &[&str] = &[
    "<html", "<body", "<div", "<table", "<td", "<p>", "<p ", "<br", "<span",
];

/// Wraps each tool so the HTML in its successful results becomes Markdown.
pub(crate) fn wrap(tools: Vec<Arc<dyn Tool>>) -> Vec<Arc<dyn Tool>> {
    tools
        .into_iter()
        .map(|tool| Arc::new(Markdown(tool)) as Arc<dyn Tool>)
        .collect()
}

struct Markdown(Arc<dyn Tool>);

#[async_trait]
impl Tool for Markdown {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn parameters(&self) -> Value {
        self.0.parameters()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let mut result = self.0.execute(ctx).await?;
        paging::rewrite_text(&mut result, |output| convert_output(&output));
        Ok(result)
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.0.timeout_secs()
    }

    fn max_output_bytes(&self) -> usize {
        self.0.max_output_bytes()
    }

    fn max_output_lines(&self) -> usize {
        self.0.max_output_lines()
    }

    fn humanize(&self, args: &Value) -> String {
        self.0.humanize(args)
    }

    fn concurrent(&self) -> bool {
        self.0.concurrent()
    }
}

/// The output with its HTML as Markdown, or `None` when it has no HTML.
fn convert_output(output: &str) -> Option<String> {
    match serde_json::from_str::<Value>(output) {
        Ok(mut value) => convert_value(&mut value).then(|| value.to_string()),
        Err(_) => is_html(output).then(|| to_markdown(output)),
    }
}

/// Converts every HTML string under `value` and says whether there was one.
/// An object whose `contentType` said `html` says `markdown` after.
fn convert_value(value: &mut Value) -> bool {
    match value {
        Value::String(text) if is_html(text) => {
            *text = to_markdown(text);
            true
        }
        // Counted rather than `any`, which would stop at the first one.
        Value::Array(items) => {
            items
                .iter_mut()
                .map(convert_value)
                .filter(|&converted| converted)
                .count()
                > 0
        }
        Value::Object(fields) => {
            let converted = fields
                .values_mut()
                .map(convert_value)
                .filter(|&converted| converted)
                .count()
                > 0;
            if converted
                && let Some(kind) = fields.get_mut("contentType")
                && kind
                    .as_str()
                    .is_some_and(|kind| kind.eq_ignore_ascii_case("html"))
            {
                *kind = Value::from("markdown");
            }
            converted
        }
        _ => false,
    }
}

fn is_html(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    HTML_TAGS.iter().any(|tag| lower.contains(tag))
}

/// Raw mode lays out the tables emails use for their layout as plain blocks
/// instead of drawing them. Blank runs collapse to one blank line.
fn to_markdown(html: &str) -> String {
    let Ok(markdown) = html2text::config::plain()
        .raw_mode(true)
        .string_from_read(html.as_bytes(), WIDTH)
    else {
        return html.to_owned();
    };
    let mut out = String::with_capacity(markdown.len());
    let mut blank = false;
    for line in markdown.lines().map(str::trim_end) {
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.truncate(out.trim_end().len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const EMAIL: &str = "<html><head><style>p{margin:0}</style></head><body><table><tr><td>\
        <p>Go to the <b>furthest security line on the right</b>.</p><br>\
        <p><a href=\"https://example.com/event\">View Event</a></p></td></tr></table></body></html>";

    #[test]
    fn outlook_body_becomes_markdown() {
        let output = json!({"value": [{"subject": "Arrival", "body": {"contentType": "html", "content": EMAIL}}]});
        let converted: Value =
            serde_json::from_str(&convert_output(&output.to_string()).unwrap()).unwrap();
        let body = &converted["value"][0]["body"];
        assert_eq!(body["contentType"], "markdown");
        let content = body["content"].as_str().unwrap();
        assert!(content.contains("Go to the **furthest security line on the right**."));
        assert!(content.contains("[View Event][1]"));
        assert!(content.contains("[1]: https://example.com/event"));
        assert!(!content.contains("margin"));
        assert_eq!(converted["value"][0]["subject"], "Arrival");
    }

    #[test]
    fn text_output_that_is_html_becomes_markdown() {
        let converted = convert_output(EMAIL).unwrap();
        assert!(converted.starts_with("Go to the **furthest"));
        assert!(!converted.contains("\n\n\n"));
    }

    #[test]
    fn output_without_html_is_left_alone() {
        assert_eq!(convert_output(r#"{"preview": "a < b and c > d"}"#), None);
        assert_eq!(convert_output("No results"), None);
    }
}
