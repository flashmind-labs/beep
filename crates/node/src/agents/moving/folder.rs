//! A folder as a transcript names it: a path, or a `file://` URI.

use std::{
    fmt::Write,
    path::{Path, PathBuf},
};

/// A session's folders before and after moving between machines.
#[derive(Clone)]
pub(super) struct Folders {
    pub(super) old: PathBuf,
    pub(super) new: PathBuf,
    /// The original checkout, when the session later entered a worktree.
    pub(super) workspace: Option<Workspace>,
}

#[derive(Clone)]
pub(super) struct Workspace {
    pub(super) checkout: PathBuf,
    pub(super) old: PathBuf,
    pub(super) new: PathBuf,
}

impl Folders {
    pub(super) fn remap(&self, value: &str) -> Option<String> {
        match &self.workspace {
            Some(workspace) => remap(value, &workspace.old, &workspace.new)
                .or_else(|| remap(value, &workspace.checkout, &workspace.new)),
            None => remap(value, &self.old, &self.new),
        }
    }
}

/// `value` with the folder `old` swapped for `new`, when it is `old` or
/// inside it; `None` for any other value.
fn remap(value: &str, old: &Path, new: &Path) -> Option<String> {
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

#[cfg(test)]
#[path = "../../tests/agents_moving_folder.rs"]
mod tests;
