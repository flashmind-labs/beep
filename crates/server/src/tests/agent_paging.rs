use super::*;
use serde_json::json;

fn text(text: &str) -> ContentPart {
    ContentPart::Text { text: text.into() }
}

fn image() -> ContentPart {
    ContentPart::ImageUrl { url: "u".into() }
}

fn parts(result: &ToolResult) -> Vec<Value> {
    serde_json::to_value(result.content())
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn json_parts(parts: Vec<ContentPart>) -> Vec<Value> {
    serde_json::to_value(parts)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn rewrite_merges_text_and_keeps_media_after() {
    let mut result = ToolResult::success_with_content("c", vec![text("a"), image(), text("b")]);
    rewrite_text(&mut result, |old| Some(old.to_uppercase()));
    assert_eq!(parts(&result), json_parts(vec![text("AB"), image()]));
}

#[test]
fn rewrite_leaves_media_only_results_alone() {
    let mut result = ToolResult::success_with_content("c", vec![image()]);
    rewrite_text(&mut result, |old| Some(old + "x"));
    assert_eq!(parts(&result), json_parts(vec![image()]));
}

fn marked(value: Value) -> bool {
    mark(value.to_string()).starts_with(MORE)
}

#[test]
fn each_app_s_next_page_is_seen() {
    assert!(marked(json!({"hasMore": true, "results": []})));
    assert!(marked(
        json!({"data": {"messages": [], "nextPageToken": "abc"}})
    ));
    assert!(marked(
        json!({"data": {"@odata.nextLink": "https://example.com/next"}})
    ));
    assert!(marked(
        json!({"data": {"messages": {"paging": {"page": 1, "pages": 3}}}})
    ));
}

#[test]
fn a_last_page_is_left_alone() {
    assert!(!marked(json!({"hasMore": false, "nextCursor": null})));
    assert!(!marked(json!({"data": {"nextPageToken": ""}})));
    assert!(!marked(json!({"paging": {"page": 1, "pages": 1}})));
    assert!(!marked(
        json!({"a": {"b": {"c": {"d": {"hasMore": true}}}}})
    ));
    assert_eq!(mark("not json".into()), "not json");
}
