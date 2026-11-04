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
        while let Some(c) = self
            .peek()
            .filter(|c| c.is_ascii_alphanumeric() || "-.".contains(*c))
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
        while let Some(c) = self
            .peek()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
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
mod tests {
    use super::*;

    fn keeps_only(config: &str) -> String {
        kept(config.as_bytes())
    }

    const GOOD: &str = "[core]\n\
        \trepositoryformatversion = 0\n\
        \tfilemode = true\n\
        \tbare = false\n\
        \tlogallrefupdates = true\n\
        [remote \"origin\"]\n\
        \turl = git@example.com:team/app.git\n\
        \tfetch = +refs/heads/*:refs/remotes/origin/*\n\
        [branch \"main\"]\n\
        \tremote = origin\n\
        \tmerge = refs/heads/main\n";

    #[test]
    fn the_keys_a_checkout_needs_stay() {
        let kept = keeps_only(GOOD);
        for line in [
            "repositoryformatversion = \"0\"",
            "filemode = \"true\"",
            "url = \"git@example.com:team/app.git\"",
            "fetch = \"+refs/heads/*:refs/remotes/origin/*\"",
            "merge = \"refs/heads/main\"",
        ] {
            assert!(kept.contains(line), "{line} missing from\n{kept}");
        }
        // Reading what it wrote gives the same again.
        assert_eq!(keeps_only(&kept), kept);
    }

    #[test]
    fn keys_that_make_git_run_a_program_are_dropped() {
        let hostile = [
            "[core]\n\tfsmonitor = /tmp/payload\n",
            "[core]\n\tsshCommand = /tmp/payload\n",
            "[core]\n\thooksPath = /tmp/hooks\n",
            "[core]\n\teditor = /tmp/payload\n\tpager = /tmp/payload\n",
            "[alias]\n\tx = !/tmp/payload\n",
            "[filter \"lfs\"]\n\tclean = /tmp/payload\n\tsmudge = /tmp/payload\n",
            "[diff \"x\"]\n\ttextconv = /tmp/payload\n",
            "[credential]\n\thelper = !/tmp/payload\n",
            "[include]\n\tpath = /tmp/more\n",
            "[includeIf \"gitdir:/\"]\n\tpath = /tmp/more\n",
            "[url \"/tmp/x\"]\n\tinsteadOf = https://example.com/\n",
            "[remote \"origin\"]\n\tpushurl = ext::sh -c payload\n\tuploadpack = /tmp/payload\n",
            "[extensions]\n\tworktreeConfig = true\n\tobjectformat = sha999\n\trefstorage = /tmp/x\n",
            "[extensions]\n\tpartialClone = origin\n\tcompatobjectformat = sha1\n",
        ];
        for config in hostile {
            assert_eq!(keeps_only(config), "", "{config}");
        }
    }

    #[test]
    fn case_variants_do_not_get_past() {
        for config in [
            "[CORE]\n\tFsMonitor = /tmp/payload\n",
            "[Core]\n\tSSHCOMMAND = /tmp/payload\n",
            "[core]\n\tHOOKSPATH = /tmp/hooks\n",
            "[ALIAS]\n\tx = !payload\n",
            "[Include]\n\tPath = /tmp/more\n",
            "[core]\nfsmonitor=/tmp/payload\n",
            "[core] fsmonitor = /tmp/payload\n",
            "[core]\n\tbare = false\n\tfsmonitor = a \\\n b\n",
        ] {
            let kept = keeps_only(config);
            assert!(
                !kept.to_lowercase().contains("fsmonitor")
                    && !kept.to_lowercase().contains("sshcommand")
                    && !kept.to_lowercase().contains("hookspath")
                    && !kept.to_lowercase().contains("include")
                    && !kept.to_lowercase().contains("alias")
                    && !kept.contains("payload"),
                "{config} kept\n{kept}"
            );
        }
        // The same key in another case is the same key, so it stays.
        assert!(keeps_only("[CORE]\n\tFileMode = false\n").contains("filemode"));
    }

    #[test]
    fn a_program_cannot_hide_in_a_kept_key() {
        for url in [
            "ext::sh -c payload",
            "EXT::sh -c payload",
            "fd::7",
            "-oProxyCommand=payload",
            "--upload-pack=payload",
            "foo::bar",
        ] {
            let config = format!(
                "[remote \"origin\"]\n\turl = {url}\n[branch \"main\"]\n\tremote = {url}\n"
            );
            assert_eq!(keeps_only(&config), "", "{url}");
        }
        for url in [
            "https://example.com/a.git",
            "git@example.com:a/b.git",
            "ssh://host/a.git",
            "/srv/git/a.git",
            "../a.git",
            "C:/work/a.git",
        ] {
            let config = format!("[remote \"origin\"]\n\turl = {url}\n");
            assert!(keeps_only(&config).contains("url ="), "{url}");
        }
        // A core value is one word, so it cannot carry a command.
        assert!(!keeps_only("[core]\n\tfilemode = a; /tmp/payload\n").contains("payload"));
        assert_eq!(keeps_only("[core]\n\tfilemode = a /tmp/payload\n"), "");
        assert_eq!(keeps_only("[core]\n\tfilemode = \"x y\"\n"), "");
    }

    #[test]
    fn a_line_that_does_not_parse_does_not_end_the_rest() {
        let config = "[core\n\tfsmonitor = /tmp/payload\n[remote \"o\"]\n\turl = https://e/a\n\t= x\n\tbad value\n\tfetch = +a:b\n";
        let kept = keeps_only(config);
        assert!(kept.contains("url = \"https://e/a\""), "{kept}");
        assert!(kept.contains("fetch = \"+a:b\""), "{kept}");
        assert!(!kept.contains("fsmonitor"), "{kept}");
        // Bytes that are not text still give a config.
        assert_eq!(kept_bytes(b"\xff\xfe[core]\n\tfsmonitor = x\n"), "");
    }

    #[test]
    fn the_repository_format_stays_when_it_names_a_known_one() {
        for (key, value) in [
            ("objectformat", "sha1"),
            ("objectformat", "sha256"),
            ("objectformat", "SHA256"),
            ("refstorage", "files"),
            ("refstorage", "reftable"),
        ] {
            let kept = keeps_only(&format!("[extensions]\n\t{key} = {value}\n"));
            assert!(
                kept.contains(&format!("{key} = \"{value}\"")),
                "{key} {value}: {kept}"
            );
        }
        for (key, value) in [
            ("objectformat", "sha999"),
            ("objectformat", ""),
            ("objectformat", "sha256 /tmp/x"),
            ("refstorage", "reftable2"),
            ("refstorage", "/tmp/store"),
        ] {
            let kept = keeps_only(&format!("[extensions]\n\t{key} = \"{value}\"\n"));
            assert_eq!(kept, "", "{key} {value}");
        }
    }

    fn git_ok(dir: &std::path::Path, args: &[&str]) -> Option<String> {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
            .args(args)
            .output()
            .expect("git runs");
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    /// Git opens a repository whose config went through `kept`, in its own
    /// object and ref format.
    #[test]
    fn git_opens_a_repository_with_its_config_rewritten() {
        for (flags, show, want) in [
            (
                vec!["--object-format=sha256"],
                "--show-object-format",
                "sha256",
            ),
            (
                vec!["--ref-format=reftable"],
                "--show-ref-format",
                "reftable",
            ),
        ] {
            let source = tempfile::tempdir().expect("a temp folder");
            let mut init = vec!["init", "-q"];
            init.extend(flags);
            if git_ok(source.path(), &init).is_none() {
                // This git does not know the format.
                continue;
            }
            let config = std::fs::read(source.path().join(".git/config")).expect("a config");
            let rewritten = kept(&config);
            assert!(
                rewritten.contains("repositoryformatversion = \"1\""),
                "{rewritten}"
            );
            assert!(rewritten.contains("[extensions]"), "{rewritten}");
            // A copy of the repository with only the rewritten config.
            let copy = tempfile::tempdir().expect("a temp folder");
            let mut dirs = vec![source.path().join(".git")];
            let mut at = 0;
            while at < dirs.len() {
                for entry in std::fs::read_dir(&dirs[at]).expect("a folder") {
                    let path = entry.expect("an entry").path();
                    let relative = path.strip_prefix(source.path()).expect("inside");
                    if path.is_dir() {
                        std::fs::create_dir_all(copy.path().join(relative)).expect("made");
                        dirs.push(path);
                    } else {
                        std::fs::copy(&path, copy.path().join(relative)).expect("copied");
                    }
                }
                at += 1;
            }
            std::fs::write(copy.path().join(".git/config"), &rewritten).expect("written");
            assert_eq!(
                git_ok(copy.path(), &["rev-parse", show]).as_deref(),
                Some(want)
            );
            assert!(git_ok(copy.path(), &["status", "--porcelain"]).is_some());
        }
    }

    fn kept_bytes(raw: &[u8]) -> String {
        kept(raw)
    }
}
