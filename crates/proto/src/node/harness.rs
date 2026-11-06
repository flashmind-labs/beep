//! The coding agent harnesses on a node (Claude Code, Codex, pi, ...): what
//! the node finds of each, asked with `SessionsRequest::Harnesses` on
//! `agents/1`, and the user's settings for them, which the server sends
//! with `Down::AgentSettings` and the node applies when it starts one.
//!
//! Like the rest of the node link these are compact msgpack, so a field is
//! added only at the end with `#[serde(default)]`.

pub use output::{clean_output, failure_line};

use super::EnvSecret;
use serde::{Deserialize, Serialize};

/// A harness Beep knows: where it keeps its home and how it installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Harness {
    /// Its slug, the agent's name on the node, such as `claude`.
    pub agent: &'static str,
    /// The variable that moves its home and config folder.
    pub home_var: &'static str,
    /// Its home folder when that variable is not set.
    pub home_default: &'static str,
    /// The npm package of its CLI, when it ships on npm.
    pub npm: Option<&'static str>,
}

/// The harnesses Beep knows, in the order the apps list them.
pub const HARNESSES: &[Harness] = &[
    Harness {
        agent: "claude",
        home_var: "CLAUDE_CONFIG_DIR",
        home_default: "~/.claude",
        npm: Some("@anthropic-ai/claude-code"),
    },
    Harness {
        agent: "codex",
        home_var: "CODEX_HOME",
        home_default: "~/.codex",
        npm: Some("@openai/codex"),
    },
    Harness {
        agent: "pi",
        home_var: "PI_CODING_AGENT_DIR",
        home_default: "~/.pi/agent",
        npm: Some("@mariozechner/pi-coding-agent"),
    },
    Harness {
        agent: "gemini",
        home_var: "GEMINI_CLI_HOME",
        home_default: "~",
        npm: Some("@google/gemini-cli"),
    },
    Harness {
        agent: "flsh",
        home_var: "FLASHMIND_HOME",
        home_default: "~/.flashmind",
        npm: None,
    },
    Harness {
        agent: "opencode",
        home_var: "OPENCODE_CONFIG_DIR",
        home_default: "~/.config/opencode",
        npm: Some("opencode-ai"),
    },
];

/// The harness `agent` names, when Beep knows it.
pub fn harness(agent: &str) -> Option<&'static Harness> {
    HARNESSES.iter().find(|harness| harness.agent == agent)
}

/// What a node found of each harness, for `SessionsReply::Harnesses`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HarnessReport {
    pub harnesses: Vec<HarnessInfo>,
    /// When the newest versions were last looked up, in Unix seconds; 0
    /// when never.
    pub checked_at: u64,
}

/// One harness on a node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HarnessInfo {
    pub agent: String,
    /// Its CLI is on the node's `PATH`.
    pub installed: bool,
    /// The node can start it: the CLI and the ACP adapter it needs are
    /// there. Only these are offered.
    pub runnable: bool,
    /// Its version, such as `2.1.312`, as its `--version` says.
    pub version: Option<String>,
    /// The newest version published, when it could be looked up.
    pub latest: Option<String>,
    /// Where the CLI is.
    pub cli_path: Option<String>,
    /// What the node starts for a session, with the user's settings left
    /// out: the CLI itself or its ACP adapter.
    pub program: Option<String>,
    /// Whether it is signed in, when that can be read on the machine.
    pub signed_in: Option<bool>,
    /// Who it is signed in as, when it says.
    pub account: Option<String>,
    /// The plan of that account, such as `max`, when it says.
    pub plan: Option<String>,
    /// The command `UpdateHarness` runs: an install while it is missing,
    /// an update after. `None` when the node knows of none.
    pub install: Option<String>,
}

/// The user's settings for the harnesses on one machine, sent in full
/// after the welcome and whenever they change. A harness with none runs
/// as the node finds it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub harnesses: Vec<HarnessConfig>,
    /// The user's flsh settings, the same on every machine, which the node
    /// writes to flsh's config before it starts flsh. `None` until the
    /// user saves some, and from a server before them: the node then
    /// leaves flsh's config alone.
    pub flsh: Option<crate::FlshSettings>,
}

/// How the node starts one harness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessConfig {
    pub agent: String,
    /// Off: the node refuses to start it and the server does not offer it.
    #[serde(default = "on")]
    pub enabled: bool,
    /// The program to start in place of the one the node found.
    #[serde(default)]
    pub program: Option<String>,
    /// The folder its home variable (`Harness::home_var`) is set to.
    #[serde(default)]
    pub home: Option<String>,
    /// Added after the node's own arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Set on its process, over everything else.
    #[serde(default)]
    pub env: Vec<EnvSecret>,
    /// When the node compacts its sessions. `None` is the default: on, at
    /// `AutoCompact::DEFAULT_PERCENT`.
    #[serde(default)]
    pub compact: Option<AutoCompact>,
}

/// Whether and when the node compacts a harness's sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoCompact {
    /// Off: the node never sends `/compact`.
    pub enabled: bool,
    /// Compact once the conversation fills this percent of the window, 1 to
    /// 100. `None` is `DEFAULT_PERCENT`.
    #[serde(default)]
    pub percent: Option<u8>,
    /// Compact once the conversation reaches this many tokens. When set it
    /// wins over `percent`. Added last: a node that predates it reads
    /// `percent` alone.
    #[serde(default)]
    pub tokens: Option<u64>,
}

impl AutoCompact {
    /// The percent used when none is set.
    pub const DEFAULT_PERCENT: u8 = 80;

    /// Whether `percent` is one the node accepts.
    pub fn valid_percent(percent: u8) -> bool {
        (1..=100).contains(&percent)
    }
}

fn on() -> bool {
    true
}

impl HarnessConfig {
    /// The settings of a harness nobody changed.
    pub fn new(agent: &str) -> Self {
        Self {
            agent: agent.to_owned(),
            enabled: true,
            program: None,
            home: None,
            args: Vec::new(),
            env: Vec::new(),
            compact: None,
        }
    }
}

/// The version in what a CLI's `--version` printed, such as `2.1.312` in
/// `2.1.312 (Claude Code)` or `codex-cli 0.166.0`: the first word that
/// starts with a digit (after an optional `v`) and has a dot in it.
pub fn parse_version(output: &str) -> Option<String> {
    output.split_whitespace().find_map(|word| {
        let word = word.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';' | '"'));
        let word = word.strip_prefix('v').unwrap_or(word);
        let starts = word.chars().next().is_some_and(|c| c.is_ascii_digit());
        let clean = word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
        (starts && clean && word.contains('.')).then(|| word.to_owned())
    })
}

/// Whether `latest` is newer than `current`, comparing the dotted numbers
/// of each; a pre-release (`1.2.0-beta`) counts as its numbers.
pub fn is_newer(latest: &str, current: &str) -> bool {
    fn numbers(version: &str) -> Vec<u64> {
        version
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    }
    let (latest, current) = (numbers(latest), numbers(current));
    let len = latest.len().max(current.len());
    let at = |list: &[u64], i: usize| list.get(i).copied().unwrap_or(0);
    for i in 0..len {
        match at(&latest, i).cmp(&at(&current, i)) {
            std::cmp::Ordering::Greater => return true,
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    false
}

mod output {
    //! Turning what an install or update command printed into text a person
    //! can read: terminal escapes, progress bars and spinners taken out, and
    //! the lines that say why it failed picked from the end.

    const ESC: char = '\u{1b}';
    const BEL: char = '\u{7}';
    /// The one-character C1 forms of CSI and OSC.
    const CSI: char = '\u{9b}';
    const OSC: char = '\u{9d}';
    /// Left where an escape was taken out, so a line that held only escapes
    /// is told from a blank one. Never in the text otherwise: control
    /// characters are dropped.
    const MARK: char = '\u{1}';

    /// `text` as a person reads it: ANSI and OSC escapes removed, each line
    /// as a terminal leaves it after carriage returns overwrote it, progress
    /// glyphs dropped, lines left with no words dropped, repeats and runs of
    /// blank lines folded into one.
    pub fn clean_output(text: &str) -> String {
        let plain = strip_escapes(skip_cut_escape(text));
        let mut lines: Vec<String> = Vec::new();
        for raw in plain.split('\n') {
            let escaped = raw.contains(MARK);
            let raw = raw.replace(MARK, "");
            let line = match last_overwrite(&raw) {
                Some(line) => line,
                None => {
                    // A blank line stays; one that held only escapes goes.
                    if !escaped {
                        lines.push(String::new());
                    }
                    continue;
                }
            };
            let had_glyphs = line.chars().any(is_progress_glyph);
            let line: String = line.chars().filter(|c| !is_progress_glyph(*c)).collect();
            let line = if had_glyphs {
                line.split_whitespace().collect::<Vec<_>>().join(" ")
            } else {
                line.trim_end().to_owned()
            };
            if !line.chars().any(char::is_alphabetic) {
                // Only bars, glyphs or numbers.
                continue;
            }
            if lines.last() == Some(&line) {
                continue;
            }
            lines.push(line);
        }
        let mut out = String::new();
        let mut blank = false;
        for line in lines {
            if line.is_empty() {
                blank = !out.is_empty();
                continue;
            }
            if !out.is_empty() {
                out.push_str(if blank { "\n\n" } else { "\n" });
            }
            blank = false;
            out.push_str(&line);
        }
        out
    }

    /// The lines of `clean` (as `clean_output` gives it) that say why the
    /// command failed, on one line of at most about `max` characters: from
    /// the earliest error line that still fits to the end, or else the start
    /// of the last error line, or else the last words.
    pub fn failure_line(clean: &str, max: usize) -> String {
        let lines: Vec<String> = clean
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|line| !line.is_empty())
            .collect();
        let joined = |from: usize| lines[from..].join(" ");
        let errors: Vec<usize> = (0..lines.len())
            .filter(|&i| mentions_error(&lines[i]))
            .collect();
        if let Some(&last) = errors.last() {
            if let Some(&from) = errors
                .iter()
                .find(|&&from| joined(from).chars().count() <= max)
            {
                return joined(from);
            }
            return head(&joined(last), max);
        }
        tail(&joined(0), max)
    }

    /// Whether `line` reads as an error.
    fn mentions_error(line: &str) -> bool {
        let lower = line.to_lowercase();
        ["error", "err!", "fatal", "failed", "denied", "not found"]
            .iter()
            .any(|word| lower.contains(word))
    }

    /// The first `max` characters of `text`, cut at a space.
    fn head(text: &str, max: usize) -> String {
        if text.chars().count() <= max {
            return text.to_owned();
        }
        let cut: String = text.chars().take(max).collect();
        let cut = match cut.rfind(' ') {
            Some(space) if space > max / 2 => &cut[..space],
            _ => &cut,
        };
        format!("{}…", cut.trim_end())
    }

    /// The last `max` characters of `text`, cut at a space.
    fn tail(text: &str, max: usize) -> String {
        let count = text.chars().count();
        if count <= max {
            return text.to_owned();
        }
        let cut: String = text.chars().skip(count - max).collect();
        let cut = match cut.find(' ') {
            Some(space) if space < max / 2 => &cut[space + 1..],
            _ => &cut,
        };
        format!("…{}", cut.trim_start())
    }

    /// `text` without the rest of an SGR escape whose start was cut off, such
    /// as the `238m` left when output is cut to its last characters.
    fn skip_cut_escape(text: &str) -> &str {
        let digits = text
            .find(|c: char| !(c.is_ascii_digit() || c == ';'))
            .unwrap_or(text.len());
        if digits > 0 && text[digits..].starts_with('m') {
            &text[digits + 1..]
        } else {
            text
        }
    }

    /// `text` without its ANSI escapes (CSI, OSC, DCS and the short ones) and
    /// control characters other than newline, tab and carriage return.
    fn strip_escapes(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                ESC => match chars.next() {
                    Some('[') => {
                        skip_csi(&mut chars);
                        out.push(MARK);
                    }
                    Some(']' | 'P' | 'X' | '^' | '_') => {
                        skip_string(&mut chars);
                        out.push(MARK);
                    }
                    // Charset and other short escapes: intermediates, then one
                    // final character.
                    Some(' '..='/') => {
                        while chars.next_if(|c| (' '..='/').contains(c)).is_some() {}
                        chars.next();
                        out.push(MARK);
                    }
                    _ => out.push(MARK),
                },
                CSI => {
                    skip_csi(&mut chars);
                    out.push(MARK);
                }
                OSC => {
                    skip_string(&mut chars);
                    out.push(MARK);
                }
                '\n' | '\t' | '\r' => out.push(c),
                c if c.is_control() => {}
                c => out.push(c),
            }
        }
        out
    }

    /// Past a CSI sequence's parameters, intermediates and final byte.
    fn skip_csi(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
        while chars.next_if(|c| ('0'..='?').contains(c)).is_some() {}
        while chars.next_if(|c| (' '..='/').contains(c)).is_some() {}
        chars.next_if(|c| ('@'..='~').contains(c));
    }

    /// Past an OSC (or DCS) string, which BEL or ESC `\` ends.
    fn skip_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
        while let Some(c) = chars.next() {
            match c {
                BEL => return,
                ESC => {
                    chars.next_if_eq(&'\\');
                    return;
                }
                _ => {}
            }
        }
    }

    /// What a terminal shows of `line` after its carriage returns: the last
    /// part with something in it. `None` when it is blank.
    fn last_overwrite(line: &str) -> Option<&str> {
        line.split('\r').rev().find(|part| !part.trim().is_empty())
    }

    /// Block elements, shades and braille spinners: progress bars and logos.
    fn is_progress_glyph(c: char) -> bool {
        matches!(c, '\u{2580}'..='\u{259f}' | '\u{2800}'..='\u{28ff}' | '\u{25a0}'..='\u{25ff}')
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The end of what a failed `opencode upgrade` printed, cut to its last
        /// characters as an older node did: the tail of its logo in colored
        /// blocks, then its error.
        const OPENCODE: &str = "238m \u{1b}[0m\u{1b}[0m\u{2588}\u{1b}[0m\u{1b}[0m\u{2588}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m\u{1b}[0m\u{1b}[48;5;238m\u{2580}\u{1b}[0m \u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m \u{1b}[90m\u{2588}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m\u{1b}[38;5;235m\u{2580}\u{1b}[0m\u{1b}[38;5;235m\u{2580}\u{1b}[0m\u{1b}[90m\u{2580}\u{1b}[0m \u{1b}[0m\u{2580}\u{1b}[0m\u{1b}[0m\u{2580}\u{1b}[0m\u{1b}[0m\u{2580}\u{1b}[0m \u{1b}[0m \u{1b}[91m\u{1b}[1mError: \u{1b}[0mUnexpected error, check log file at /path/to/home/.local/share/opencode/log/2026-11-03T112330.log for more details SyntaxError: JSON Parse error: Unexpected EOF";

        #[test]
        fn the_opencode_sample_comes_down_to_its_error() {
            let clean = clean_output(OPENCODE);
            let expected = "Error: Unexpected error, check log file at /path/to/home/.local/share/opencode/log/2026-11-03T112330.log for more details SyntaxError: JSON Parse error: Unexpected EOF";
            assert_eq!(clean, expected);
            assert_eq!(failure_line(&clean, 300), expected);
        }

        #[test]
        fn the_same_error_on_its_own_lines_keeps_both() {
            let text = "\u{1b}[48;5;238m\u{2580}\u{2580}\u{2580}\u{1b}[0m\n\u{1b}[90m\u{2588}\u{2588}\u{1b}[0m\n\n\u{1b}[91m\u{1b}[1mError: \u{1b}[0mUnexpected error, check log file\nSyntaxError: JSON Parse error: Unexpected EOF\n";
            let clean = clean_output(text);
            assert_eq!(
                clean,
                "Error: Unexpected error, check log file\nSyntaxError: JSON Parse error: Unexpected EOF"
            );
            assert_eq!(
                failure_line(&clean, 300),
                "Error: Unexpected error, check log file SyntaxError: JSON Parse error: Unexpected EOF"
            );
        }

        #[test]
        fn progress_bars_and_spinners_are_dropped() {
            let text = "Downloading\r[####      ] 40%\r[##########] 100%\n\
                \u{280b} Resolving\r\u{2819} Resolving\r\u{2839} Resolving\n\
                \u{2593}\u{2593}\u{2593}\u{2591}\u{2591} 60%\n\
                \u{1b}]0;npm install\u{7}\u{1b}[2K\u{1b}[1G\n\
                \u{1b}[?25l\u{1b}[?25h\n\
                added 3 packages in 2s\n\
                added 3 packages in 2s\n";
            assert_eq!(clean_output(text), "Resolving\nadded 3 packages in 2s");
        }

        #[test]
        fn the_failure_line_is_cut_to_length() {
            let long = format!("Error: {}", "word ".repeat(100));
            let line = failure_line(&long, 60);
            assert!(line.starts_with("Error: word"), "{line}");
            assert!(line.ends_with('…'), "{line}");
            assert!(line.chars().count() <= 61, "{line}");
            // Without an error line, the last words are kept.
            let words = (0..100)
                .map(|i| format!("w{i}"))
                .collect::<Vec<_>>()
                .join(" ");
            let line = failure_line(&words, 30);
            assert!(line.starts_with('…') && line.ends_with("w99"), "{line}");
            // The earliest error line that fits starts the line.
            let text = "Downloading formula\nError: refusing to load\nRun brew trust to trust it.";
            assert_eq!(
                failure_line(text, 300),
                "Error: refusing to load Run brew trust to trust it."
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SecretText;

    #[test]
    fn versions_are_read_from_what_each_cli_prints() {
        assert_eq!(
            parse_version("2.1.312 (Claude Code)").as_deref(),
            Some("2.1.312")
        );
        assert_eq!(
            parse_version("codex-cli 0.166.0").as_deref(),
            Some("0.166.0")
        );
        assert_eq!(parse_version("v0.99.2\n").as_deref(), Some("0.99.2"));
        assert_eq!(
            parse_version("flsh 0.41.0-beta.1").as_deref(),
            Some("0.41.0-beta.1")
        );
        assert_eq!(parse_version("opencode"), None);
        assert_eq!(parse_version("error: 42 things"), None);
    }

    #[test]
    fn newer_compares_numbers_not_text() {
        assert!(is_newer("2.1.320", "2.1.312"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.2"));
        assert!(!is_newer("2.1.312", "2.1.312"));
        assert!(!is_newer("2.1.3", "2.1.312"));
        assert!(!is_newer("1.2.0-beta", "1.2.0"));
    }

    #[test]
    fn settings_round_trip_hide_their_values_and_default_to_on() {
        let settings = AgentSettings {
            harnesses: vec![HarnessConfig {
                env: vec![EnvSecret {
                    name: "HTTPS_PROXY".into(),
                    value: SecretText::new("http://user:pass7777@proxy"),
                }],
                args: vec!["--verbose".into()],
                ..HarnessConfig::new("codex")
            }],
            flsh: Some(crate::FlshSettings {
                zdr: Some(true),
                quantizations: vec![crate::FlshQuantization::Fp8],
                sort: Some(crate::FlshSort::Throughput),
                models: vec![crate::FlshModelRule {
                    model: "openrouter:vendor/model".parse().unwrap(),
                    autocompact: crate::FlshAutoCompact {
                        enabled: true,
                        tokens: Some(120_000),
                    },
                }],
                ..crate::FlshSettings::default()
            }),
        };
        let body = rmp_serde::to_vec_named(&settings).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<AgentSettings>(&body).unwrap(),
            settings
        );
        assert!(!format!("{settings:?}").contains("7777"));
        #[derive(Serialize)]
        struct Older {
            agent: &'static str,
        }
        let body = rmp_serde::to_vec_named(&Older { agent: "pi" }).unwrap();
        let read: HarnessConfig = rmp_serde::from_slice(&body).unwrap();
        assert_eq!(read, HarnessConfig::new("pi"));
    }
}
