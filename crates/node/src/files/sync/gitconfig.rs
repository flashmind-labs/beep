//! What a peer's `.git/config` may bring. A config can make git run a program
//! (`core.fsmonitor`, `core.sshCommand`, `core.hooksPath`, aliases, filter and
//! diff drivers, credential helpers, `ext::` remotes), so a config from a
//! peer is rewritten before it lands: only the keys that say where the
//! remotes and branches are, and how the repository behaves on disk, are kept.

use std::fmt::Write;

/// The keys kept, as `section.key`, and whether the section needs a subsection.
const KEPT: [(&str, bool); 12] = [
    ("remote.url", true),
    ("remote.fetch", true),
    ("branch.remote", true),
    ("branch.merge", true),
    ("core.repositoryformatversion", false),
    ("core.bare", false),
    ("core.ignorecase", false),
    ("core.precomposeunicode", false),
    ("core.filemode", false),
    ("core.logallrefupdates", false),
    ("extensions.objectformat", false),
    ("extensions.refstorage", false),
];

/// The largest config read; a bigger one is not brought in.
pub(super) const MAX_BYTES: u64 = 1 << 20;

/// The longest value of a `core` key, which are all one word.
const CORE_VALUE: usize = 16;

/// `raw` with only the kept keys, written out plainly. Whatever does not
/// parse is dropped.
pub(super) fn kept(raw: &[u8]) -> String {
    let text: Vec<char> = String::from_utf8_lossy(raw).chars().collect();
    let mut scan = Scan { text: &text, at: 0 };
    let mut out = String::new();
    let mut open: Option<Section> = None;
    let mut written: Option<Section> = None;

    while let Some(c) = scan.skip_blank() {
        match c {
            '#' | ';' => scan.skip_line(),
            '[' => {
                open = scan.header();

                if open.is_none() {
                    scan.skip_line();
                }
            }
            _ => {
                let Some(name) = scan.name() else {
                    scan.skip_line();
                    continue;
                };
                let Some(value) = scan.value() else {
                    scan.skip_line();
                    continue;
                };
                let Some(section) = &open else { continue };
                let name = name.to_ascii_lowercase();

                if !keeps(section, &name, &value) {
                    continue;
                }

                if written.as_ref() != Some(section) {
                    section.write(&mut out);
                    written = Some(section.clone());
                }
                // Writing to a string cannot fail.
                let _ = writeln!(out, "\t{name} = {}", quoted(&value));
            }
        }
    }

    out
}

#[derive(Clone, PartialEq)]
struct Section {
    /// Lower case, as git compares it.
    name: String,
    /// Case matters here.
    sub: Option<String>,
}

impl Section {
    fn write(&self, out: &mut String) {
        // Writing to a string cannot fail.
        let _ = match &self.sub {
            Some(sub) => writeln!(out, "[{} \"{}\"]", self.name, escape(sub)),
            None => writeln!(out, "[{}]", self.name),
        };
    }
}

fn keeps(section: &Section, key: &str, value: &str) -> bool {
    let full = format!("{}.{key}", section.name);
    let Some((_, needs_sub)) = KEPT.iter().find(|(kept, _)| *kept == full) else {
        return false;
    };

    if section.sub.as_ref().is_some_and(|sub| !sub.is_empty()) != *needs_sub
        || value.chars().any(char::is_control)
    {
        return false;
    }

    match full.as_str() {
        // Where a remote or a branch's remote points: a plain address, never
        // an option for git or a program for a transport helper to run.
        "remote.url" | "branch.remote" => !value.is_empty() && !is_unsafe_address(value),
        "remote.fetch" | "branch.merge" => !value.is_empty(),
        // The repository's format, which only these words name.
        "extensions.objectformat" => {
            ["sha1", "sha256"].contains(&value.to_ascii_lowercase().as_str())
        }
        "extensions.refstorage" => {
            ["files", "reftable"].contains(&value.to_ascii_lowercase().as_str())
        }
        _ => {
            value.len() <= CORE_VALUE
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        }
    }
}

/// Whether git would take `address` for an option, or hand it to a
/// `git-remote-<name>` helper (`ext::` runs a command).
fn is_unsafe_address(address: &str) -> bool {
    let address = address.trim_start();
    address.starts_with('-')
        || address
            .split_once("::")
            .is_some_and(|(helper, _)| !helper.is_empty() && !helper.contains(['/', '\\']))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn quoted(value: &str) -> String {
    let mut out = String::from("\"");

    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c => out.push(c),
        }
    }

    out.push('"');
    out
}

struct Scan<'a> {
    text: &'a [char],
    at: usize,
}

impl Scan<'_> {
    fn peek(&self) -> Option<char> {
        self.text.get(self.at).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += 1;
        Some(c)
    }

    /// Past whitespace and line ends; the next character, not taken.
    fn skip_blank(&mut self) -> Option<char> {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }

        self.peek()
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.next() {
            if c == '\n' {
                break;
            }
        }
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(|c| c == ' ' || c == '\t') {
            self.at += 1;
        }
    }

    /// `[name]`, `[name "sub"]` or the older `[name.sub]`.
    fn header(&mut self) -> Option<Section> {
        self.next();
        let mut name = String::new();

        while let Some(c) = self.peek()
            && (c.is_ascii_alphanumeric() || "-.".contains(c))
        {
            name.push(c);
            self.at += 1;
        }

        self.skip_spaces();
        let mut sub = None;

        if self.peek() == Some('"') {
            self.at += 1;
            let mut text = String::new();

            loop {
                match self.next()? {
                    '"' => break,
                    '\n' => return None,
                    '\\' => text.push(self.next().filter(|c| *c != '\n')?),
                    c => text.push(c),
                }
            }
            sub = Some(text);
        }

        if self.next()? != ']' || name.is_empty() {
            return None;
        }

        let name = name.to_ascii_lowercase();
        Some(match (sub, name.split_once('.')) {
            (None, Some((name, sub))) => Section {
                name: name.to_owned(),
                sub: Some(sub.to_owned()),
            },
            (sub, _) => Section { name, sub },
        })
    }

    /// A variable's name: a letter, then letters, digits and dashes.
    fn name(&mut self) -> Option<String> {
        let mut name = String::new();

        while let Some(c) = self.peek()
            && (c.is_ascii_alphanumeric() || c == '-')
        {
            name.push(c);
            self.at += 1;
        }

        name.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            .then_some(name)
    }

    /// What follows a name: `= value`, or nothing for true.
    fn value(&mut self) -> Option<String> {
        self.skip_spaces();

        match self.peek() {
            Some('=') => {
                self.at += 1;
                self.text_of_value()
            }
            None | Some('\n' | '\r' | '#' | ';') => Some("true".to_owned()),
            Some(_) => None,
        }
    }

    /// The value as git reads it: quotes group, a backslash escapes or joins
    /// lines, `#` and `;` outside quotes start a comment. The line end is
    /// taken.
    fn text_of_value(&mut self) -> Option<String> {
        let mut value = String::new();
        let (mut quoted, mut comment, mut space) = (false, false, false);

        loop {
            let c = match self.next() {
                None if quoted => return None,
                None => break,
                Some('\r') if self.peek() == Some('\n') => continue,
                Some(c) => c,
            };

            if c == '\n' {
                if quoted {
                    return None;
                }
                break;
            }

            if comment {
                continue;
            }

            if c.is_whitespace() && !quoted {
                space = !value.is_empty();
                continue;
            }

            if !quoted && (c == '#' || c == ';') {
                comment = true;
                continue;
            }

            if space {
                value.push(' ');
                space = false;
            }
            match c {
                '\\' => match self.next()? {
                    '\n' => {}
                    '\r' if self.peek() == Some('\n') => {
                        self.at += 1;
                    }
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'b' => value.push('\u{8}'),
                    c @ ('"' | '\\') => value.push(c),
                    _ => return None,
                },
                '"' => quoted = !quoted,
                c => value.push(c),
            }
        }

        Some(value)
    }
}

#[cfg(test)]
#[path = "../../tests/files_sync_gitconfig.rs"]
mod tests;
