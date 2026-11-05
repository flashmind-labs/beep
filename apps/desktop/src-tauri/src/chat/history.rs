//! A chat's messages as the bar shows them (`Turn`), read from the local
//! copy or the server, and a tool result as a step of a reply (`tool_step`).

use crate::{
    beep::{Beep, Field},
    chat::{cards, markdown, products, tool_labels},
    error::Result,
};
use beep_proto::{
    Chart, GetSession, GetSessionImages, GetToolLabels, HistoryEntry, ImageRef, SentImages,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use tokio::{join, time::timeout};
use uuid::Uuid;

/// How long a chat read from the copy waits for its tool labels.
const LABELS_WAIT: Duration = Duration::from_millis(500);
/// The server saves follow-up replies as this tool's result.
pub const SUGGEST_REPLIES: &str = "suggest_replies";
/// Its result names a file Beep sent, which shows as a card.
const SEND_FILE: &str = "send_file";
/// Its result lists pictures, from the web or kept with the chat.
const SEND_IMAGE: &str = "send_image";
/// Its result lists the picture it made, the same as `send_image`'s.
const IMAGE_GEN: &str = "image_gen";
/// Their results name the video or 3D model they made, the same as
/// `send_file`'s.
const MADE_FILES: [&str; 2] = ["video_gen", "3d_gen"];
/// Their results hold a chart, drawn under the reply.
const SHOW_CHART: &str = "show_chart";
const SHOW_STOCK_CHART: &str = "show_stock_chart";
const GET_ROUTE: &str = "get_route";
/// Runs coding agents; its result describes a session, which the bar shows
/// as a card.
const CODE_AGENT: &str = "code_agent";
/// Its result lists products, which show as a row of cards.
const SHOW_PRODUCTS: &str = "show_products";
/// Its result lists recommendations, which show as a row of cards.
const SHOW_CARDS: &str = "show_cards";

/// One message of a chat, as the bar shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Turn {
    User {
        text: String,
        /// When it was sent, in milliseconds: the `message` that
        /// `session_files` lists its files by.
        at: i64,
        /// The pictures sent with it, read with the `picture` command.
        images: Vec<ImageRef>,
        /// The model the turn ran on, empty when the server did not say.
        model: String,
        /// The thinking level, known only when Auto picked it.
        thinking: String,
    },
    Assistant {
        html: String,
        tools: Vec<ToolStep>,
        replies: Vec<String>,
        /// The model that wrote it, empty when the server did not say.
        model: String,
    },
    Scheduled {
        label: String,
    },
    Approval {
        query_id: String,
        app: String,
        action: String,
        fields: Vec<Field>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolStep {
    pub name: String,
    /// What the step showed while it ran, from `GetToolLabels`. For a step
    /// the server kept none for, what a tool that takes an `action` did,
    /// read from its result.
    pub label: Option<String>,
    /// The call's id, to match the step with its `GetToolLabels` entry.
    #[serde(skip)]
    call_id: String,
    pub success: bool,
    pub file: Option<SentFile>,
    /// A `send_image` result's pictures.
    pub images: Vec<SentImage>,
    /// A `show_products` result's cards, as HTML.
    pub products: Option<String>,
    /// A `show_cards` result's cards, as HTML.
    pub cards: Option<String>,
    /// A `show_chart` or `show_stock_chart` result's chart.
    pub chart: Option<Chart>,
    /// A `get_route` call's id, to fetch its transit card with `GetRoute`.
    pub route: Option<String>,
    /// A `code_agent` result's text, which names the session the bar shows
    /// a card for.
    pub code_agent: Option<String>,
}

impl ToolStep {
    /// A step of a chat kept on this Mac: what it showed, and whether it worked.
    pub(crate) fn local(call_id: String, name: String, label: String, success: bool) -> Self {
        Self {
            name,
            label: Some(label),
            call_id,
            success,
            file: None,
            images: Vec::new(),
            products: None,
            cards: None,
            chart: None,
            route: None,
            code_agent: None,
        }
    }
}

/// A file Beep sent in the chat, fetched with `GetFile` when opened.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentFile {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// A picture Beep sent with `send_image`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentImage {
    #[serde(flatten)]
    pub source: ImageSource,
    #[serde(default)]
    pub caption: String,
}

/// Where a sent picture comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    /// An https image on the web.
    Url(String),
    /// An image kept with the chat, fetched like a sent file.
    File(SentFile),
}

/// The pictures a chat's messages carried, by the message they came with:
/// the user entry's timestamp in a session, the message's seq in a group.
pub struct Pictures(HashMap<i64, Vec<ImageRef>>);

impl Pictures {
    /// The pictures listed in `reply`. A server too old to keep pictures
    /// fails the call, which only means there are none.
    pub fn new(reply: Result<Vec<SentImages>>) -> Self {
        let sent = reply.unwrap_or_else(|e| {
            log::info!("listing pictures: {e}");
            Vec::new()
        });
        Self(sent.into_iter().map(|s| (s.message, s.images)).collect())
    }

    /// The pictures `message` carried, if any.
    pub fn take(&mut self, message: i64) -> Vec<ImageRef> {
        self.0.remove(&message).unwrap_or_default()
    }
}

impl Beep {
    pub async fn history(&self, id: String) -> Result<Vec<Turn>> {
        let session_id = Uuid::parse_str(&id).ok();
        if let Some(turns) = session_id.and_then(|id| self.chats().session(id)) {
            // The copy shows at once: the labels only name tool steps
            // better, so a slow or absent server does not hold it back.
            let labels = timeout(LABELS_WAIT, self.tool_labels(session_id))
                .await
                .unwrap_or_else(|_| {
                    log::debug!("tool labels of {id} were slow; showing the chat without them");
                    HashMap::new()
                });
            return Ok(with_labels(turns, labels));
        }
        let (entries, images, labels) = join!(
            self.call(GetSession { id: id.clone() }),
            self.call(GetSessionImages { session_id: id }),
            self.tool_labels(session_id),
        );
        let mut pictures = Pictures::new(images);
        let turns = entries?
            .into_iter()
            .map(|entry| turn(entry, &mut pictures))
            .collect();
        Ok(with_labels(turns, labels))
    }

    /// What each tool step of chat `session_id` showed while it ran, by
    /// call id. Empty from a server that does not keep them, and then the
    /// steps keep the labels read from their results.
    async fn tool_labels(&self, session_id: Option<Uuid>) -> HashMap<String, String> {
        let Some(session_id) = session_id else {
            return HashMap::new();
        };
        match self.call(GetToolLabels { session_id }).await {
            Ok(labels) => labels
                .into_iter()
                .map(|label| (label.call_id, label.description))
                .collect(),
            Err(e) => {
                log::debug!("tool labels of {session_id}: {e}");
                HashMap::new()
            }
        }
    }
}

#[derive(Deserialize)]
struct Suggestions {
    replies: Vec<String>,
}

/// The replies in a `suggest_replies` result, or `None` when it has none.
pub fn suggested_replies(output: &str) -> Option<Vec<String>> {
    let suggestions: Suggestions = serde_json::from_str(output).ok()?;
    let replies: Vec<String> = suggestions
        .replies
        .into_iter()
        .filter(|reply| !reply.trim().is_empty())
        .collect();
    (!replies.is_empty()).then_some(replies)
}

/// A tool result as a step of a reply.
pub fn tool_step(name: &str, call_id: &str, output: &str, success: bool) -> ToolStep {
    ToolStep {
        route: (name == GET_ROUTE && success && !call_id.is_empty()).then(|| call_id.to_owned()),
        name: name.to_owned(),
        label: tool_labels::action_label(name, output).map(str::to_owned),
        call_id: call_id.to_owned(),
        success,
        file: sent_file(name, output),
        images: sent_images(name, output),
        products: (name == SHOW_PRODUCTS)
            .then(|| products::shown(output))
            .flatten(),
        cards: (name == SHOW_CARDS).then(|| cards::shown(output)).flatten(),
        chart: shown_chart(name, output),
        // The text of a `code_agent` call that went through.
        code_agent: (name == CODE_AGENT && success).then(|| output.to_owned()),
    }
}

#[derive(Deserialize)]
struct FileOutput {
    file: SentFile,
}

/// The file a `send_file`, `video_gen` or `3d_gen` result names, or `None`
/// for any other tool.
fn sent_file(name: &str, output: &str) -> Option<SentFile> {
    if name != SEND_FILE && !MADE_FILES.contains(&name) {
        return None;
    }
    serde_json::from_str::<FileOutput>(output)
        .ok()
        .map(|o| o.file)
}

#[derive(Deserialize)]
struct ImagesOutput {
    images: Vec<SentImage>,
}

/// The pictures a `send_image` or `image_gen` result lists, or none for any
/// other tool.
fn sent_images(name: &str, output: &str) -> Vec<SentImage> {
    if name != SEND_IMAGE && name != IMAGE_GEN {
        return Vec::new();
    }
    serde_json::from_str::<ImagesOutput>(output)
        .map(|o| o.images)
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct ChartOutput {
    chart: Chart,
}

/// The chart a `show_chart` or `show_stock_chart` result draws, or `None`
/// for any other tool.
fn shown_chart(name: &str, output: &str) -> Option<Chart> {
    if name != SHOW_CHART && name != SHOW_STOCK_CHART {
        return None;
    }
    serde_json::from_str::<ChartOutput>(output)
        .ok()
        .map(|o| o.chart)
        .filter(|chart| chart.series.iter().any(|s| !s.points.is_empty()))
}

/// `turns` with each tool step labelled as it showed while it ran, where
/// `labels` has its call.
fn with_labels(mut turns: Vec<Turn>, labels: HashMap<String, String>) -> Vec<Turn> {
    if labels.is_empty() {
        return turns;
    }
    for turn in &mut turns {
        let Turn::Assistant { tools, .. } = turn else {
            continue;
        };
        for step in tools {
            if let Some(label) = labels.get(&step.call_id) {
                step.label = Some(label.clone());
            }
        }
    }
    turns
}

fn turn(entry: HistoryEntry, pictures: &mut Pictures) -> Turn {
    match entry {
        HistoryEntry::User {
            content,
            timestamp,
            auto,
        } => Turn::User {
            text: content,
            at: timestamp,
            images: pictures.take(timestamp),
            model: auto
                .as_ref()
                .map(|pick| pick.model.clone())
                .unwrap_or_default(),
            thinking: auto
                .map(|pick| pick.thinking.to_string())
                .unwrap_or_default(),
        },
        HistoryEntry::Assistant {
            content,
            tools,
            model,
            ..
        } => {
            let (suggest, tools): (Vec<_>, Vec<_>) =
                tools.into_iter().partition(|t| t.name == SUGGEST_REPLIES);
            Turn::Assistant {
                html: markdown::to_html(&content),
                tools: tools
                    .iter()
                    .map(|t| tool_step(&t.name, &t.call_id, &t.output, t.success))
                    .collect(),
                replies: suggest
                    .iter()
                    .find_map(|t| suggested_replies(&t.output))
                    .unwrap_or_default(),
                model,
            }
        }
        HistoryEntry::Scheduled { label, .. } => Turn::Scheduled { label },
        HistoryEntry::Approval { query_id, approval } => Turn::Approval {
            query_id,
            app: approval.app,
            action: approval.action,
            fields: approval.fields.into_iter().map(Field::from).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_labels_prefers_the_live_description() {
        let turns = vec![Turn::Assistant {
            html: String::new(),
            tools: vec![
                tool_step("memory", "c1", r#"{"stored":true}"#, true),
                tool_step("memory", "c2", r#"{"stored":true}"#, true),
            ],
            replies: Vec::new(),
            model: String::new(),
        }];
        let labels = HashMap::from([("c1".to_owned(), "Remembering your flight".to_owned())]);
        let Turn::Assistant { tools, .. } = &with_labels(turns, labels)[0] else {
            panic!("an assistant turn stays one");
        };
        assert_eq!(tools[0].label.as_deref(), Some("Remembering your flight"));
        assert_eq!(tools[1].label.as_deref(), Some("Remembering"));
    }

    #[test]
    fn sent_file_reads_send_file_results_only() {
        let output = r#"{"action":"send_file","file":{"id":"f1","name":"Pass.pdf","mime":"application/pdf","size":26000}}"#;
        let file = sent_file(SEND_FILE, output).expect("a send_file result names its file");
        assert_eq!(file.id, "f1");
        assert_eq!(file.size, 26000);
        assert!(sent_file("web_search", output).is_none());
        assert!(sent_file(SEND_FILE, "failed").is_none());
    }

    #[test]
    fn sent_images_reads_urls_and_kept_files() {
        let output = r#"{"action":"send_image","images":[{"url":"https://pics.example/a.jpg","caption":"A"},{"file":{"id":"f2","name":"chart.png","mime":"image/png","size":900},"caption":""}],"failed":[]}"#;
        let images = sent_images(SEND_IMAGE, output);
        assert_eq!(images.len(), 2);
        assert!(
            matches!(&images[0].source, ImageSource::Url(url) if url == "https://pics.example/a.jpg")
        );
        assert_eq!(images[0].caption, "A");
        assert!(matches!(&images[1].source, ImageSource::File(file) if file.id == "f2"));
        assert!(sent_images(SEND_FILE, output).is_empty());
        assert!(sent_images(SEND_IMAGE, "failed").is_empty());
    }

    #[test]
    fn made_media_shows_like_sent_media() {
        let image = r#"{"action":"send_image","images":[{"file":{"id":"f3","name":"cat.png","mime":"image/png","size":900},"caption":""}],"provider":"OpenAI","model":"gpt-image-2","path":"cat.png"}"#;
        let images = sent_images("image_gen", image);
        assert_eq!(images.len(), 1);
        assert!(matches!(&images[0].source, ImageSource::File(file) if file.id == "f3"));
        let model = r#"{"action":"send_file","file":{"id":"f4","name":"cat.glb","mime":"model/gltf-binary","size":4000},"provider":"Meshy","model":"latest","path":"cat.glb"}"#;
        for tool in ["video_gen", "3d_gen"] {
            let file = sent_file(tool, model).expect("a made file is named like a sent one");
            assert_eq!(file.id, "f4");
        }
    }

    #[test]
    fn shown_products_reads_show_products_results_only() {
        let output = r#"{"action":"show_products","products":[{"title":"Kettle","url":"https://shop.example/k","store":"shop.example","price":19.5,"currency":"USD"}]}"#;
        let row = tool_step(SHOW_PRODUCTS, "c1", output, true)
            .products
            .expect("a show_products result has cards");
        assert!(row.contains("<b>$19.50</b>"), "{row}");
        assert!(tool_step(SEND_FILE, "c1", output, true).products.is_none());
        assert!(
            tool_step(SHOW_PRODUCTS, "c1", "failed", true)
                .products
                .is_none()
        );
    }

    #[test]
    fn shown_chart_reads_chart_results_only() {
        let output = r#"{"action":"show_stock_chart","chart":{"title":"AAPL","kind":"area","x":"time","categories":[],"unit":"USD","series":[{"name":"AAPL","points":[{"x":1.0,"y":2.0}]}],"stock":null,"source":"Yahoo Finance"}}"#;
        let chart = shown_chart(SHOW_STOCK_CHART, output).expect("a chart result has a chart");
        assert_eq!(chart.series[0].points.len(), 1);
        assert!(shown_chart(SHOW_PRODUCTS, output).is_none());
        assert!(shown_chart(SHOW_CHART, "failed").is_none());
    }
}
