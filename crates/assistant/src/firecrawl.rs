//! Web search and page reading through Firecrawl's v2 API
//! (`POST /search`, `POST /scrape`, bearer key).

use crate::{AssistantError, Endpoint};
use reqwest::Client;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};

const SEARCH_LIMIT: u32 = 5;
/// Characters of each result's page kept in a search.
const RESULT_CHARS: usize = 1500;

#[derive(Deserialize)]
struct Reply<T> {
    data: Option<T>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Found {
    #[serde(default)]
    web: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    url: Option<String>,
    title: Option<String>,
    description: Option<String>,
    markdown: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    markdown: Option<String>,
}

async fn post<T: for<'de> Deserialize<'de>>(
    client: &Client,
    endpoint: &Endpoint,
    path: &str,
    body: Value,
) -> Result<T, AssistantError> {
    let response = client
        .post(format!("{}/{path}", endpoint.base))
        .bearer_auth(endpoint.key.expose_secret())
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    let reply: Reply<T> = serde_json::from_str(&text)
        .map_err(|_| AssistantError::Api(format!("Firecrawl answered {status}.")))?;
    match reply.data {
        Some(data) if status.is_success() => Ok(data),
        _ => Err(AssistantError::Api(
            reply
                .error
                .unwrap_or_else(|| format!("Firecrawl answered {status}.")),
        )),
    }
}

/// The top results for `query` as plain text.
pub(crate) async fn search(
    client: &Client,
    endpoint: &Endpoint,
    query: &str,
) -> Result<String, AssistantError> {
    let body = json!({
        "query": query,
        "limit": SEARCH_LIMIT,
        "scrapeOptions": {"formats": [{"type": "markdown"}]},
    });
    let found: Found = post(client, endpoint, "search", body).await?;
    if found.web.is_empty() {
        return Ok("No results.".into());
    }
    let mut out = String::new();
    for (i, hit) in found.web.iter().enumerate() {
        out.push_str(&format!(
            "{}. {}\n{}\n",
            i + 1,
            hit.title.as_deref().unwrap_or("(no title)"),
            hit.url.as_deref().unwrap_or("")
        ));
        let body = hit
            .markdown
            .as_deref()
            .or(hit.description.as_deref())
            .unwrap_or("");
        out.push_str(&crate::tools::cap(body, RESULT_CHARS));
        out.push_str("\n\n");
    }
    Ok(out)
}

/// The main content of the page at `url` as markdown.
pub(crate) async fn fetch(
    client: &Client,
    endpoint: &Endpoint,
    url: &str,
) -> Result<String, AssistantError> {
    let body = json!({"url": url, "formats": ["markdown"], "onlyMainContent": true});
    let page: Page = post(client, endpoint, "scrape", body).await?;
    Ok(page
        .markdown
        .unwrap_or_else(|| "The page had no text.".into()))
}
