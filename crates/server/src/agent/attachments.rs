//! Files the user hands Beep, shown to the model.

use flashmind::types::ContentPart;

/// Most characters of a text file shown to the model inline.
const MAX_INLINE: usize = 40_000;

/// Types that are text without a `text/` prefix.
const TEXT_TYPES: &[&str] = &[
    "application/json",
    "application/xml",
    "application/yaml",
    "application/x-yaml",
    "application/toml",
    "application/javascript",
    "application/x-sh",
    "application/sql",
    "application/x-ndjson",
    "application/ld+json",
];

/// A file to show the model.
pub struct Handed<'a> {
    pub name: &'a str,
    pub mime: &'a str,
    pub data: &'a [u8],
}

impl Handed<'_> {
    /// The file as text, when it is text.
    fn text(&self) -> Option<&str> {
        let mime = self.mime.split(';').next().unwrap_or("").trim();
        let texty = mime.starts_with("text/")
            || TEXT_TYPES.contains(&mime)
            || mime.ends_with("+json")
            || mime.ends_with("+xml");
        // Browsers give an empty or generic type for many text files, so a
        // file that decodes cleanly counts too.
        let unknown = mime.is_empty() || mime == "application/octet-stream";
        let text = std::str::from_utf8(self.data).ok()?;
        (texty || (unknown && !text.contains('\0'))).then_some(text)
    }
}

/// What the model reads of `file`.
pub fn describe(file: &Handed<'_>) -> String {
    let head = format!(
        "<file name=\"{}\" type=\"{}\" size=\"{}\">\n",
        attribute(file.name),
        attribute(file.mime),
        size(file.data.len())
    );
    let body = match file.text() {
        Some(text) if text.chars().count() <= MAX_INLINE => text.to_owned(),
        Some(text) => {
            let shown: String = text.chars().take(MAX_INLINE).collect();
            format!("{shown}\n[Cut here.]")
        }
        None => "[Its contents cannot be read here.]".to_owned(),
    };
    format!("{head}{body}\n</file>")
}

/// Whether `part` is one of the files `parts` makes.
pub fn is_file(part: &ContentPart) -> bool {
    matches!(part, ContentPart::Text { text } if text.starts_with("<file name=\""))
}

/// The parts a message's files add.
pub fn parts(files: &[Handed<'_>]) -> Vec<ContentPart> {
    files
        .iter()
        .map(|file| ContentPart::Text {
            text: describe(file),
        })
        .collect()
}

fn attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// `bytes` for reading, such as "240 KB".
fn size(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..1_048_576 => format!("{} KB", bytes / 1024),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handed<'a>(name: &'a str, mime: &'a str, data: &'a [u8]) -> Handed<'a> {
        Handed { name, mime, data }
    }

    #[test]
    fn long_text_is_cut() {
        let long = "x".repeat(MAX_INLINE + 10);
        let text = describe(&handed("big.txt", "text/plain", long.as_bytes()));
        assert!(text.contains("[Cut here."));
        assert!(text.len() < long.len() + 300);
    }

    #[test]
    fn names_cannot_break_out_of_the_tag() {
        let text = describe(&handed("a\"><b.txt", "text/plain", b"hi"));
        assert!(text.starts_with("<file name=\"a&quot;>&lt;b.txt\""));
    }
}
