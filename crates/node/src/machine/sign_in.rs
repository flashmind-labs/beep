//! `sign-in/1`: runs a coding agent's own sign-in on this machine for a
//! desktop, which shows the link and code the agent prints and passes back
//! a code the user pastes. One sign-in per tool at a time, ended after 10
//! minutes. The agent runs on pipes, not a terminal, so it works the same on
//! Windows.

use crate::{
    lock,
    machine::env::{self, locate},
    machine::spawn_env::{self, Scope},
    run::process::{self, GroupLeader, STOP_GRACE},
};
use anyhow::Result;
use beep_proto::node::{
    LinkRx, LinkTx,
    sign_in::{Request, State, Tool},
};
use beep_proto::watched;
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
                let stopped = watched::wait_for(&mut state, |state| !state.running());

                if timeout(STOP_GRACE * 3, stopped).await.is_err() {
                    let now = watched::read(&state, Clone::clone);
                    return now;
                }
            }

            lock(&SIGN_INS).retain(|entry| entry.tool != tool);
            changed(tool);
            return State::Idle {};
        }
        None => {}
    }

    watched::read(&state, Clone::clone)
}

fn changed(tool: Tool) {
    let hub = crate::events::system();
    hub.changed(beep_proto::node::events::Topic::keyed(
        "sign-in",
        tool.to_string(),
    ));
    hub.changed(beep_proto::node::events::Topic::new("accounts"));
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
            Some(entry) if watched::read(&entry.state, State::running) => entry.state.clone(),
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
                log::info!("{tool} sign-in started");
                tokio::spawn(run(tool, program, state_tx, orders_rx));
                state
            }
        }
    };
    changed(tool);
    let wait = watched::wait_for(&mut state, |state| !matches!(state, State::Starting {}));
    // `run` gives up on its own after `LINK_LIMIT`.
    if timeout(LINK_LIMIT + STOP_GRACE * 2, wait).await.is_err() {
        return State::Starting {};
    }

    watched::read(&state, Clone::clone)
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
        // It has no sign-in command: print mode signs in first, offering
        // its link only when it streams JSON, and `/usage` then answers
        // without a model turn.
        Tool::Agy => (
            "agy",
            &["-p", "/usage", "--output-format", "stream-json"],
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
    let signed_in = ended.is_ok();
    state.send_replace(match ended {
        Ok(()) => State::SignedIn {},
        Err(reason) => State::Failed { reason },
    });
    changed(tool);

    // Probed again, so its Sign-ins row turns signed in.
    if signed_in {
        env::changed();
    }
}

async fn drive(
    tool: Tool,
    program: Option<PathBuf>,
    state: &watch::Sender<State>,
    mut orders: mpsc::Receiver<Order>,
) -> Result<(), String> {
    let (name, args) = command(tool);
    let Some(program) = program else {
        log::info!("{tool} sign-in failed: {name} is not installed");
        return Err(format!("{name} is not installed on this machine."));
    };
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
        .map_err(|e| {
            log::info!("{tool} sign-in failed: {name} could not start: {e}");
            format!("{name} could not start: {e}")
        })?;
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
                        log::info!("{tool} sign-in: the sign-in link was offered");
                        state.send_replace(prompt);
                        changed(tool);
                    }
                }
                None => output_open = false,
            },
            exit = child.child_mut().wait() => {
                // The status is logged, never the output: it can hold a link or code.
                match &exit {
                    Ok(status) => log::info!("{tool} sign-in: {name} exited with {status}"),
                    Err(e) => log::info!("{tool} sign-in: waiting for {name} failed: {e}"),
                }
                break match exit {
                    Ok(status) if status.success() && shown => {
                        log::info!("{tool} sign-in ended: signed in");
                        Ok(())
                    }
                    _ => {
                        log::info!("{tool} sign-in ended: failed, {name} did not sign in");
                        Err(read::failure(name, &seen))
                    }
                };
            }
            order = orders.recv() => match order {
                Some(Order::Answer(text)) => {
                    log::info!("{tool} sign-in: a code was received from the caller");

                    if let Err(e) = give(stdin.as_mut(), &text).await {
                        log::warn!("{tool} sign-in: {e}");
                    }
                }
                Some(Order::Cancel) => {
                    log::info!("{tool} sign-in ended: cancelled by the caller");
                    break Err("The sign-in was cancelled.".into());
                }
                None => {
                    log::info!("{tool} sign-in ended: the caller went away");
                    break Err("The sign-in was cancelled.".into());
                }
            },
            () = sleep_until(link_by) => {
                log::info!(
                    "{tool} sign-in ended: failed, {}",
                    if shown { "not finished in time" } else { "no sign-in link printed" }
                );
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
mod read;

#[cfg(all(test, unix))]
#[path = "../tests/machine_sign_in.rs"]
mod tests;
