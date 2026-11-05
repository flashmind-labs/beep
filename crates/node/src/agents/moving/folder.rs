//! A folder as a transcript names it: a path, or a `file://` URI.

use std::{fmt::Write, path::Path};

/// `value` with the folder `old` swapped for `new`, when it is `old` or
/// inside it; `None` for any other value.
pub(super) fn remap(value: &str, old: &Path, new: &Path) -> Option<String> {
    match value.strip_prefix("file://") {
        Some(rest) => {
            let moved = under(&decode(rest)?, old, new)?;
            Some(format!("file://{}", encode(&moved)))
        }
        None => under(value, old, new),
    }
}

fn under(path: &str, old: &Path, new: &Path) -> Option<String> {
    let rest = Path::new(path).strip_prefix(old).ok()?;
    let moved = if rest.as_os_str().is_empty() {
        new.to_path_buf()
    } else {
        new.join(rest)
    };
    Some(moved.to_str()?.to_owned())
}

fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
