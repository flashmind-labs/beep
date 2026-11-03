//! Reading a tool call's JSON arguments.

use serde_json::Value;

/// The string argument `key`, trimmed; a blank one is none.
pub(crate) fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
