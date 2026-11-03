//! The web pages a reply used, which the apps show as chips under it
//! instead of the model listing them in its text.

use crate::protocol::{MAX_SOURCES, WebSource};
use flashmind::types::event::Source;
use reqwest::Url;

/// How a web tool came by its sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Found {
    /// `web_scrape`: a page the model read.
    Read,
    /// `web_search`: a result it saw a snippet of.
    Listed,
}

impl Found {
    /// What `tool`'s sources are, `None` for a tool whose sources a reply
    /// does not show.
    fn by(tool: &str) -> Option<Self> {
        match tool {
            "web_scrape" => Some(Self::Read),
            "web_search" => Some(Self::Listed),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Entry {
    /// The URL without its fragment or a trailing slash, which name the
    /// same page.
    key: String,
    url: Url,
    title: String,
    found: Found,
}

/// The sources of one run's web tools, gathered as their results come.
#[derive(Debug, Default)]
pub struct ReplySources {
    entries: Vec<Entry>,
}

impl ReplySources {
    /// Adds what a call of `tool` returned. A page seen again keeps its
    /// first place, counts as read once any call read it, and takes a title
    /// when it had none.
    pub fn add(&mut self, tool: &str, sources: &[Source]) {
        let Some(found) = Found::by(tool) else {
            return;
        };
        for source in sources {
            let Some(url) = web_url(&source.url) else {
                continue;
            };
            let key = page_key(&url);
            let title = source.title.as_deref().unwrap_or("").trim();
            match self.entries.iter_mut().find(|entry| entry.key == key) {
                Some(entry) => {
                    if found == Found::Read {
                        entry.found = Found::Read;
                    }
                    if entry.title.is_empty() {
                        entry.title = title.to_owned();
                    }
                }
                None => self.entries.push(Entry {
                    key,
                    url,
                    title: title.to_owned(),
                    found,
                }),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What the reply shows: pages read first, then search results, each in
    /// the order they came, at most `MAX_SOURCES`.
    pub fn to_wire(&self) -> Vec<WebSource> {
        let read = self.entries.iter().filter(|e| e.found == Found::Read);
        let listed = self.entries.iter().filter(|e| e.found == Found::Listed);
        read.chain(listed)
            .take(MAX_SOURCES)
            .map(|entry| WebSource {
                url: entry.url.to_string(),
                title: entry.title.chars().take(300).collect(),
            })
            .collect()
    }
}

/// `url` when it is an http(s) URL with a host, short enough to keep.
fn web_url(url: &str) -> Option<Url> {
    if url.len() > 2048 {
        return None;
    }
    let url = Url::parse(url.trim()).ok()?;
    (matches!(url.scheme(), "https" | "http") && url.host_str().is_some()).then_some(url)
}

fn page_key(url: &Url) -> String {
    let mut page = url.clone();
    page.set_fragment(None);
    let text = page.to_string();
    match text.strip_suffix('/') {
        Some(trimmed) => trimmed.to_owned(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(url: &str, title: Option<&str>) -> Source {
        Source {
            url: url.to_owned(),
            title: title.map(str::to_owned),
        }
    }

    #[test]
    fn read_pages_come_before_search_results() {
        let mut sources = ReplySources::default();
        sources.add(
            "web_search",
            &[
                source("https://a.example/", Some("A")),
                source("https://b.example/events", Some("B")),
            ],
        );
        sources.add(
            "web_scrape",
            &[source("https://b.example/events#top", None)],
        );
        assert_eq!(
            sources.to_wire(),
            vec![
                WebSource {
                    url: "https://b.example/events".into(),
                    title: "B".into()
                },
                WebSource {
                    url: "https://a.example/".into(),
                    title: "A".into()
                },
            ]
        );
    }

    #[test]
    fn other_tools_and_unsafe_links_are_left_out() {
        let mut sources = ReplySources::default();
        sources.add("memory", &[source("https://memo.example/", None)]);
        sources.add(
            "web_search",
            &[
                source("javascript:alert(1)", None),
                source("ftp://x.example/", None),
            ],
        );
        assert!(sources.is_empty());
    }
}
