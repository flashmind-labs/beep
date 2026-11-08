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
