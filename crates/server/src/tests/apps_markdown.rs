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
fn output_without_html_is_left_alone() {
    assert_eq!(convert_output(r#"{"preview": "a < b and c > d"}"#), None);
    assert_eq!(convert_output("No results"), None);
}
