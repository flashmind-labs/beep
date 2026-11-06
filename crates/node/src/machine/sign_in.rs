//! `sign-in/1`: runs a coding agent's own sign-in on this machine for a
//! desktop, which shows the link and code the agent prints and passes back
//! a code the user pastes. One sign-in per tool at a time, ended after 10
//! minutes. The agent runs on pipes, not a terminal, so it works the same on
//! Windows.

use crate::{
    agents::locate,
    lock,
    machine::env,
    machine::provision::spawn_env::{self, Scope},
    run::process::{self, GroupLeader, STOP_GRACE},
};
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    sign_in::{Request, State, Tool},
};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{LazyLock, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::ChildStdin,
    select,
    sync::{mpsc, watch},
    time::{Instant, sleep_until, timeout},
};

/// How long a sign-in may take, from start to signed in.
const LIMIT: Duration = Duration::from_secs(10 * 60);
/// How long the agent may take to print its link.
const LINK_LIMIT: Duration = Duration::from_secs(30);
/// The most of the agent's output kept for reading its link and why it failed.
const MAX_OUTPUT: usize = 64 * 1024;
/// The longest line of the agent's passed on as the reason it failed.
const REASON_CHARS: usize = 200;
/// The longest code a user may paste.
const MAX_ANSWER: usize = 4096;

/// What the user asks of a running sign-in.
enum Order {
    Answer(String),
    Cancel,
}

/// A sign-in started on this node, running or ended.
struct Entry {
    tool: Tool,
    state: watch::Receiver<State>,
    orders: mpsc::Sender<Order>,
}

/// The sign-ins of this node, one per tool: the last one started.
static SIGN_INS: LazyLock<Mutex<Vec<Entry>>> = LazyLock::new(Mutex::default);

/// Changed each time a sign-in ends signed in, so the machine is probed
/// again and its Sign-ins row turns signed in.
static SIGNED_IN: LazyLock<watch::Sender<()>> = LazyLock::new(|| watch::channel(()).0);

/// Says when a sign-in on this machine ended signed in.
pub(crate) fn signed_in() -> watch::Receiver<()> {
    SIGNED_IN.subscribe()
}

/// Serves one `sign-in/1` stream: one state per request.
pub(crate) async fn serve(mut tx: LinkTx, mut rx: LinkRx) -> Result<()> {
    while let Some(request) = rx.recv::<Request>().await? {
        tx.send(&answer(request).await).await?;
    }
    Ok(())
}

async fn answer(request: Request) -> State {
    let (name, order) = match request {
        Request::Start { tool } => {
            return match tool.parse() {
                Ok(tool) => start(tool).await,
                Err(_) => unknown(&tool),
            };
        }
        Request::State { tool } => (tool, None),
        Request::Answer { tool, text } => (tool, Some(Order::Answer(text))),
        Request::Cancel { tool } => (tool, Some(Order::Cancel)),
        _ => {
            return State::Failed {
                reason: "This beep-node does not know that request.".into(),
            };
        }
    };
    let Ok(tool) = name.parse::<Tool>() else {
        return unknown(&name);
    };
    let found = lock(&SIGN_INS)
        .iter()
        .find(|entry| entry.tool == tool)
        .map(|entry| (entry.state.clone(), entry.orders.clone()));
    let Some((mut state, orders)) = found else {
        return State::Idle {};
    };
    match order {
        // A send fails once it ended: its state says how.
        Some(Order::Answer(text)) => {
            if orders.send(Order::Answer(text)).await.is_err() {
                log::debug!("{tool} sign-in had ended before the code came");
            }
        }
        Some(Order::Cancel) => {
            if orders.send(Order::Cancel).await.is_ok() {
                let stopped = state.wait_for(|state| !state.running());
                if timeout(STOP_GRACE * 3, stopped).await.is_err() {
                    let now = state.borrow().clone();
                    return now;
                }
            }
            lock(&SIGN_INS).retain(|entry| entry.tool != tool);
            return State::Idle {};
        }
        None => {}
    }
    state.borrow().clone()
}

fn unknown(tool: &str) -> State {
    State::Failed {
        reason: format!(
            "This beep-node cannot sign {tool} in. Update it, or sign in on that machine."
        ),
    }
}

/// Starts `tool`'s sign-in, unless one runs, and waits for its link.
async fn start(tool: Tool) -> State {
    let mut state = {
        let mut sign_ins = lock(&SIGN_INS);
        match sign_ins.iter().find(|entry| entry.tool == tool) {
            Some(entry) if entry.state.borrow().running() => entry.state.clone(),
            _ => {
                let (state_tx, state) = watch::channel(State::Starting {});
                let (orders, orders_rx) = mpsc::channel(4);
                sign_ins.retain(|entry| entry.tool != tool);
                sign_ins.push(Entry {
                    tool,
                    state: state.clone(),
                    orders,
                });
                let program = find(command(tool).0);
                tokio::spawn(run(tool, program, state_tx, orders_rx));
                state
            }
        }
    };
    let wait = state.wait_for(|state| !matches!(state, State::Starting {}));
    // `run` gives up on its own after `LINK_LIMIT`.
    if timeout(LINK_LIMIT + STOP_GRACE * 2, wait).await.is_err() {
        return State::Starting {};
    }
    state.borrow().clone()
}

/// What the node runs for `tool`.
fn command(tool: Tool) -> (&'static str, &'static [&'static str]) {
    match tool {
        Tool::Claude => ("claude", &["auth", "login"]),
        Tool::Codex => ("codex", &["login", "--device-auth"]),
        Tool::Opencode => (
            "opencode",
            &[
                "auth",
                "login",
                "--provider",
                "openai",
                "--method",
                "ChatGPT Pro/Plus (headless)",
            ],
        ),
    }
}

/// `program` as the machine probe finds it: on the node's `PATH`, or in the
/// install folders a service's `PATH` leaves out.
fn find(program: &str) -> Option<PathBuf> {
    which::which_in(program, spawn_env::search_path(), "/")
        .ok()
        .or_else(|| locate::fallback(program, &dirs::home_dir()?))
}

/// Runs one sign-in to its end, saying how it stands on `state`.
async fn run(
    tool: Tool,
    program: Option<PathBuf>,
    state: watch::Sender<State>,
    orders: mpsc::Receiver<Order>,
) {
    let ended = drive(tool, program, &state, orders).await;
    if let Err(reason) = &ended {
        log::info!("{tool} sign-in ended: {reason}");
    }
    let signed_in = ended.is_ok();
    state.send_replace(match ended {
        Ok(()) => State::SignedIn {},
        Err(reason) => State::Failed { reason },
    });
    if signed_in {
        SIGNED_IN.send_replace(());
    }
}

async fn drive(
    tool: Tool,
    program: Option<PathBuf>,
    state: &watch::Sender<State>,
    mut orders: mpsc::Receiver<Order>,
) -> Result<(), String> {
    let (name, args) = command(tool);
    let program = program.ok_or_else(|| format!("{name} is not installed on this machine."))?;
    let mut command = process::command(&program);
    // No credentials: the agent signs in on its own.
    spawn_env::apply(&mut command, Scope::Probe);
    env::beside(&mut command, &program);
    process::own_group(&mut command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|e| format!("{name} could not start: {e}"))?;
    let mut child = GroupLeader::new(child);
    let mut stdin = child.child_mut().stdin.take();
    let (output_tx, mut output) = mpsc::channel(16);
    if let Some(out) = child.child_mut().stdout.take() {
        tokio::spawn(pump(out, output_tx.clone()));
    }
    if let Some(err) = child.child_mut().stderr.take() {
        tokio::spawn(pump(err, output_tx));
    }
    let started = Instant::now();
    let mut seen = String::new();
    let mut shown = false;
    let mut output_open = true;
    let ended = loop {
        let link_by = started + if shown { LIMIT } else { LINK_LIMIT };
        select! {
            chunk = output.recv(), if output_open => match chunk {
                Some(chunk) => {
                    seen.push_str(&chunk);
                    if seen.len() > MAX_OUTPUT {
                        let mut cut = seen.len() - MAX_OUTPUT;
                        while !seen.is_char_boundary(cut) {
                            cut += 1;
                        }
                        seen.drain(..cut);
                    }
                    if !shown && let Some(prompt) = read::prompt(tool, &seen) {
                        shown = true;
                        state.send_replace(prompt);
                    }
                }
                None => output_open = false,
            },
            exit = child.child_mut().wait() => {
                break match exit {
                    Ok(status) if status.success() && shown => Ok(()),
                    _ => Err(read::failure(name, &seen)),
                };
            }
            order = orders.recv() => match order {
                Some(Order::Answer(text)) => {
                    if let Err(e) = give(stdin.as_mut(), &text).await {
                        log::warn!("{tool} sign-in: {e}");
                    }
                }
                Some(Order::Cancel) | None => break Err("The sign-in was cancelled.".into()),
            },
            () = sleep_until(link_by) => {
                break Err(if shown {
                    "The sign-in was not finished within 10 minutes.".into()
                } else {
                    format!("{name} did not print a sign-in link.")
                });
            }
        }
    };
    // Gone once it ended; stopped if it still runs.
    drop(stdin.take());
    child.stop(STOP_GRACE).await;
    ended
}

/// Writes the user's code to the agent's input, as one line.
async fn give(stdin: Option<&mut ChildStdin>, text: &str) -> std::io::Result<()> {
    let Some(stdin) = stdin else {
        return Ok(());
    };
    let text: String = text
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ANSWER)
        .collect();
    stdin.write_all(text.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await
}

/// Sends what `from` prints, in chunks as they come, until it closes.
async fn pump(mut from: impl AsyncRead + Unpin, to: mpsc::Sender<String>) {
    let mut buf = [0u8; 4096];
    loop {
        match from.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                if to
                    .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    }
}

/// Reading what the agents print. Plain functions, so they test without
/// running an agent.
mod read {
    use super::{REASON_CHARS, State, Tool};

    /// Where each agent sends the user: only these links are passed on.
    fn hosts(tool: Tool) -> &'static [&'static str] {
        match tool {
            Tool::Claude => &["claude.com", "claude.ai", "anthropic.com"],
            Tool::Codex | Tool::Opencode => &["openai.com", "chatgpt.com"],
        }
    }

    /// The link (and code) in what the agent printed so far, once the
    /// lines that hold them are complete.
    pub(super) fn prompt(tool: Tool, output: &str) -> Option<State> {
        let text = plain(output);
        // Only whole lines: the rest may still be on its way.
        let done = &text[..text.rfind('\n')?];
        let words = || done.split_whitespace();
        let url = words().find_map(|word| link(word, hosts(tool)))?;
        match tool {
            Tool::Claude => Some(State::Waiting {
                url,
                code: None,
                asks_code: true,
            }),
            Tool::Codex | Tool::Opencode => {
                let code = words()
                    .map(|word| word.trim_end_matches(['.', ',']))
                    .find(|word| is_code(word))?;
                Some(State::Waiting {
                    url,
                    code: Some(code.to_owned()),
                    asks_code: false,
                })
            }
        }
    }

    /// `word` when it is an https link to one of `hosts` or under them.
    pub(super) fn link(word: &str, hosts: &[&str]) -> Option<String> {
        let word = word.trim_matches(['.', ',', ')', '(', '"', '\'', '<', '>']);
        let rest = word.strip_prefix("https://")?;
        let host = rest
            .split(['/', '?', '#', ':'])
            .next()?
            .to_ascii_lowercase();
        let theirs = hosts
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
        let clean = word.chars().all(|c| c.is_ascii_graphic());
        (theirs && clean).then(|| word.to_owned())
    }

    /// A one-time code such as `ABCD-12345`: hyphen-joined capitals and digits.
    pub(super) fn is_code(word: &str) -> bool {
        let groups: Vec<&str> = word.split('-').collect();
        (4..=20).contains(&word.len())
            && groups.len() >= 2
            && groups.iter().all(|group| {
                !group.is_empty()
                    && group
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            })
    }

    /// What Claude Code asks with, on the line its answer then goes on.
    const CLAUDE_ASKS: &str = "Paste code here if prompted >";

    /// Why the sign-in ended: the last line the agent printed.
    pub(super) fn failure(name: &str, output: &str) -> String {
        let text = plain(output);
        let last = text
            .lines()
            .map(|line| {
                let line = line.trim();
                line.strip_prefix(CLAUDE_ASKS).map_or(line, str::trim)
            })
            .rfind(|line| !line.is_empty() && !line.starts_with("WARNING"))
            .unwrap_or_default();
        if last.is_empty() {
            return format!("{name} could not sign in.");
        }
        let short: String = last.chars().take(REASON_CHARS).collect();
        format!("{name} could not sign in: {short}")
    }

    /// `text` without terminal escapes: colours and moves (CSI), and
    /// hyperlinks and titles (OSC) with what they carry.
    pub(super) fn plain(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' => match chars.next() {
                    Some('[') => {
                        for c in chars.by_ref() {
                            if c.is_ascii_alphabetic() || c == '~' {
                                // A move to a column stands for a space.
                                if matches!(c, 'C' | 'G') {
                                    out.push(' ');
                                }
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        while let Some(c) = chars.next() {
                            if c == '\u{7}' {
                                break;
                            }
                            if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                                chars.next();
                                break;
                            }
                        }
                    }
                    _ => {}
                },
                '\r' => {}
                '\n' | '\t' => out.push(c),
                c if c.is_control() => {}
                c => out.push(c),
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const CLAUDE_URL: &str = "https://claude.com/cai/oauth/authorize?code=true&client_id=00000000-0000-0000-0000-000000000000&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Ainference&code_challenge=abc&code_challenge_method=S256&state=xyz";

        /// What `claude auth login` 2.1 prints on pipes.
        fn claude_login() -> String {
            format!(
                "Opening browser to sign in\u{2026}\nIf the browser didn't open, visit: {CLAUDE_URL}\nPaste code here if prompted > "
            )
        }

        #[test]
        fn claude_gives_its_link_and_asks_for_a_code() {
            assert_eq!(
                prompt(Tool::Claude, &claude_login()),
                Some(State::Waiting {
                    url: CLAUDE_URL.into(),
                    code: None,
                    asks_code: true,
                })
            );
            // The link's line has not ended yet.
            let half = format!("If the browser didn't open, visit: {}", &CLAUDE_URL[..60]);
            assert_eq!(prompt(Tool::Claude, &half), None);
        }

        #[test]
        fn a_hyperlink_on_a_terminal_reads_once() {
            let drawn = format!(
                "visit: \u{1b}]8;;{CLAUDE_URL}\u{7}{CLAUDE_URL}\u{1b}]8;;\u{7}\r\nPaste code here if prompted > "
            );
            let Some(State::Waiting { url, .. }) = prompt(Tool::Claude, &drawn) else {
                panic!("no prompt in {drawn:?}");
            };
            assert_eq!(url, CLAUDE_URL);
        }

        /// What codex-cli 0.166 prints, colours and all.
        const DEVICE_AUTH: &str = "\n\u{1b}[1mWelcome to Codex\u{1b}[0m [v\u{1b}[90m0.166.0\u{1b}[0m]\n\
            Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
            1. Open this link in your browser and sign in to your account\n   \
            \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m\n\n\
            2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \
            \u{1b}[94mUZIW-O7IAI\u{1b}[0m\n\n";

        #[test]
        fn codex_gives_its_page_and_code() {
            assert_eq!(
                prompt(Tool::Codex, DEVICE_AUTH),
                Some(State::Waiting {
                    url: "https://auth.openai.com/codex/device".into(),
                    code: Some("UZIW-O7IAI".into()),
                    asks_code: false,
                })
            );
            let half: String = DEVICE_AUTH
                .lines()
                .take(5)
                .map(|l| format!("{l}\n"))
                .collect();
            assert_eq!(prompt(Tool::Codex, &half), None);
        }

        #[test]
        fn opencode_gives_its_page_and_code() {
            // What opencode 1.19 prints for ChatGPT's headless method, on pipes.
            let printed = "\u{1b}[0m\n\u{250c}  Add credential\n\u{2502}\n\
                \u{25cf}  Go to: https://auth.openai.com/codex/device\n\u{2502}\n\
                \u{25cf}  Enter code: 3590-11S3E\n\u{1b}[?25l\u{2502}\n\
                \u{25d2}  Waiting for authorization\u{1b}[999D\u{1b}[J";
            assert_eq!(
                prompt(Tool::Opencode, printed),
                Some(State::Waiting {
                    url: "https://auth.openai.com/codex/device".into(),
                    code: Some("3590-11S3E".into()),
                    asks_code: false,
                })
            );
        }

        #[test]
        fn only_the_agents_own_links_are_passed_on() {
            let openai = hosts(Tool::Codex);
            assert!(link("https://auth.openai.com/codex/device", openai).is_some());
            assert!(link("(https://chatgpt.com/device).", openai).is_some());
            assert_eq!(link("http://auth.openai.com/codex/device", openai), None);
            assert_eq!(link("https://openai.com.example.net/device", openai), None);
            assert_eq!(
                link("https://example.com/claude.com", hosts(Tool::Claude)),
                None
            );
        }

        #[test]
        fn a_failure_says_the_last_line() {
            assert_eq!(
                failure(
                    "claude",
                    "Opening browser\n\u{1b}[31mOAuth error: invalid code\u{1b}[39m\n\n"
                ),
                "claude could not sign in: OAuth error: invalid code"
            );
            assert_eq!(failure("codex", ""), "codex could not sign in.");
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_tools_and_idle_ones_say_so() {
        let refused = answer(Request::Start {
            tool: "gemini".into(),
        })
        .await;
        assert!(matches!(refused, State::Failed { .. }), "{refused:?}");
        let idle = answer(Request::State {
            tool: "opencode".into(),
        })
        .await;
        assert_eq!(idle, State::Idle {});
        let cancelled = answer(Request::Cancel {
            tool: "opencode".into(),
        })
        .await;
        assert_eq!(cancelled, State::Idle {});
    }
}
