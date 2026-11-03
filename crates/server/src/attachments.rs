//! Files the user hands Beep, saved in their home folder when an environment is online.

use crate::workspace::Workspace;
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

/// Saves `file` in the user's home folder, when there is one, and returns where.
pub async fn save(workspace: Option<&Workspace>, file: &Handed<'_>) -> Option<String> {
    let workspace = workspace?;
    match workspace.save(file.name, file.data).await {
        Ok(saved) => Some(saved.path),
        Err(error) => {
            log::warn!("unable to save a handed file: {error:#}");
            None
        }
    }
}

/// What the model reads of `file`, saved in the folder as `path`.
pub fn describe(file: &Handed<'_>, path: Option<&str>) -> String {
    let mut head = format!(
        "<file name=\"{}\" type=\"{}\" size=\"{}\"",
        attribute(file.name),
        attribute(file.mime),
        size(file.data.len())
    );
    if let Some(path) = path {
        head.push_str(&format!(" path=\"{}\"", attribute(path)));
    }
    head.push_str(">\n");
    let body = match (file.text(), path) {
        (Some(text), _) if text.chars().count() <= MAX_INLINE => text.to_owned(),
        (Some(text), Some(_)) => {
            let shown: String = text.chars().take(MAX_INLINE).collect();
            format!(
                "{shown}\n[Cut here. The whole file is in the user's home folder at the path above.]"
            )
        }
        (Some(text), None) => {
            let shown: String = text.chars().take(MAX_INLINE).collect();
            format!("{shown}\n[Cut here.]")
        }
        (None, Some(_)) => {
            "[Not shown here. Open it with exec at the path above, which is relative to \
             the home folder, for example with pdftotext for a PDF or openpyxl for a spreadsheet.]"
                .to_owned()
        }
        (None, None) => "[Its contents cannot be read here.]".to_owned(),
    };
    format!("{head}{body}\n</file>")
}

/// Whether `part` is one of the files `parts` makes.
pub fn is_file(part: &ContentPart) -> bool {
    matches!(part, ContentPart::Text { text } if text.starts_with("<file name=\""))
}

/// The parts a message's files add, each saved first.
pub async fn parts(workspace: Option<&Workspace>, files: &[Handed<'_>]) -> Vec<ContentPart> {
    let mut parts = Vec::with_capacity(files.len());
    for file in files {
        let path = save(workspace, file).await;
        parts.push(ContentPart::Text {
            text: describe(file, path.as_deref()),
        });
    }
    parts
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
    fn text_files_are_shown_inline() {
        let text = describe(
            &handed("rent.csv", "text/csv", b"month,paid\nmay,1450"),
            Some("rent.csv"),
        );
        assert!(text.starts_with(
            "<file name=\"rent.csv\" type=\"text/csv\" size=\"19 bytes\" path=\"rent.csv\">"
        ));
        assert!(text.contains("may,1450"));
    }

    #[test]
    fn untyped_text_counts_but_binary_does_not() {
        assert!(handed("notes", "", b"plain words").text().is_some());
        assert!(
            handed("blob", "application/octet-stream", b"a\0b")
                .text()
                .is_none()
        );
        assert!(
            handed("a.pdf", "application/pdf", b"%PDF-1.7")
                .text()
                .is_none()
        );
    }

    #[test]
    fn other_files_point_at_the_folder() {
        let text = describe(
            &handed("Lease 2026.pdf", "application/pdf", b"%PDF"),
            Some("Lease_2026.pdf"),
        );
        assert!(text.contains("path=\"Lease_2026.pdf\""));
        assert!(text.contains("Open it with exec"));
        let text = describe(&handed("a.pdf", "application/pdf", b"%PDF"), None);
        assert!(text.contains("cannot be read"));
    }

    #[test]
    fn long_text_is_cut() {
        let long = "x".repeat(MAX_INLINE + 10);
        let text = describe(
            &handed("big.txt", "text/plain", long.as_bytes()),
            Some("big.txt"),
        );
        assert!(text.contains("[Cut here."));
        assert!(text.len() < long.len() + 300);
    }

    #[test]
    fn names_cannot_break_out_of_the_tag() {
        let text = describe(&handed("a\"><b.txt", "text/plain", b"hi"), None);
        assert!(text.starts_with("<file name=\"a&quot;>&lt;b.txt\""));
    }
}
