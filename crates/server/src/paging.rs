//! Tells the model when a tool result is only the first page. Mail and
//! message searches return a page at a time, and a model that reads one
//! page as the whole answer reports a partial list as complete.

use serde_json::Value;

/// Put before the result, where cutting a long output cannot drop it and
/// readers that skip to the JSON still find it.
const MORE: &str = "This is not every result: the tool says there are more pages. Before \
giving a count, a total or saying this is all of them, or when acting on all of them, \
fetch the next page with the cursor or page token below. Otherwise tell the user the list \
is partial.\n";

/// How deep in a result the paging fields are looked for. Composio puts
/// them under `data`, sometimes one level further down.
const MAX_DEPTH: usize = 3;

/// `output` with a note in front when it says more results are left.
pub fn mark(output: String) -> String {
    let more = output
        .find(['{', '['])
        .and_then(|start| serde_json::from_str::<Value>(&output[start..]).ok())
        .is_some_and(|value| has_more(&value, 0));
    if more {
        format!("{MORE}{output}")
    } else {
        output
    }
}

/// Whether `value` says there is another page: a `hasMore` flag (Fastmail),
/// a next page token or cursor (Gmail, Notion, Linear), an OData next link
/// (Outlook), or a page number below the page count (Slack).
fn has_more(value: &Value, depth: usize) -> bool {
    let Value::Object(map) = value else {
        return false;
    };
    let set = |key: &str| match map.get(key) {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::String(text)) => !text.is_empty(),
        _ => false,
    };
    let flagged = ["hasMore", "has_more", "has_next_page"]
        .into_iter()
        .any(set);
    let linked = [
        "nextPageToken",
        "next_page_token",
        "nextCursor",
        "next_cursor",
        "@odata.nextLink",
    ]
    .into_iter()
    .any(set);
    let paged = map.get("paging").is_some_and(|paging| {
        let number = |key| paging.get(key).and_then(Value::as_u64);
        matches!((number("page"), number("pages")), (Some(page), Some(pages)) if page < pages)
    });
    if flagged || linked || paged {
        return true;
    }
    depth < MAX_DEPTH && map.values().any(|value| has_more(value, depth + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
