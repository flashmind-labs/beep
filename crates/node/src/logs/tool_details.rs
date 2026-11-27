//! Bounded tool arguments and results, separate from the agent's label.

use crate::text::cut;
use serde_json::{Map, Value};
use std::io;

const MAX_DETAIL: usize = 8 * 1024;

/// `value` as compact JSON, cut to `MAX_DETAIL` bytes on a character
/// boundary, and whether it was cut. Writing stops past the limit, so a
/// large result is never written out whole.
pub(crate) fn json(value: Option<&Value>) -> (Option<String>, bool) {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return (None, false);
    };
    let mut bounded = Bounded(Vec::with_capacity(MAX_DETAIL.min(1024)));
    // The only error is `Bounded` filling up.
    let truncated = serde_json::to_writer(&mut bounded, value).is_err();
    let text = match String::from_utf8(bounded.0) {
        Ok(text) => text,
        // Cut inside a character: keep the whole ones before it.
        Err(error) => {
            let whole = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(whole);
            String::from_utf8(bytes).unwrap_or_default()
        }
    };
    (Some(text), truncated)
}

/// A buffer that takes at most `MAX_DETAIL` bytes, then fails.
struct Bounded(Vec<u8>);

impl io::Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let room = MAX_DETAIL - self.0.len();

        if bytes.len() > room {
            self.0.extend_from_slice(&bytes[..room]);
            return Err(io::ErrorKind::WriteZero.into());
        }

        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A name reported by the harness, never inferred from its prose title.
pub(crate) fn name(meta: Option<&Map<String, Value>>) -> Option<String> {
    meta?
        .get("claudeCode")?
        .get("toolName")?
        .as_str()
        .filter(|name| !name.is_empty())
        .map(|name| cut(name, 240))
}

#[cfg(test)]
#[path = "../tests/logs_tool_details.rs"]
mod tests;
