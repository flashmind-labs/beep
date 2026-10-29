//! `<node> <agent> ...`: coding-agent sessions on a node, such as
//! `build-box claude start --cwd /srv/project "fix the tests"`.

use crate::{
    app::App,
    nodes::{Follower, Online, Watched},
    protocol::node::agent::{EntryKind, ExternalSession, PlanStatus, Request, SessionInfo, Status},
    user::User,
};
use chrono::Utc;
use std::{fmt::Write, sync::Arc, time::Duration};
use uuid::Uuid;

const DEFAULT_WAIT_SECS: u64 = 90;
const MAX_WAIT_SECS: u64 = 30 * 60;
/// How long `cancel` and `close` wait for the agent to stop.
const STOP_WAIT: Duration = Duration::from_secs(10);
const DEFAULT_LAST: u32 = 20;
/// The longest transcript entry shown.
const MAX_ENTRY_SHOWN: usize = 2000;

/// The only agent whose sessions started outside Beep a node finds.
const EXTERNAL_AGENT: &str = "claude";
/// The most outside sessions `sessions --all` lists.
const EXTERNAL_LISTED: u32 = 15;

/// The chat a command runs in, for following sessions.
pub(super) struct Asker<'a> {
    pub(super) app: &'a Arc<App>,
    pub(super) user: &'a Arc<User>,
    pub(super) chat: Uuid,
    /// The node's key, as the store keeps it.
    pub(super) endpoint: &'a str,
}

impl Asker<'_> {
    /// This chat, as a session reporting to it knows it.
    fn follower(&self, node: &str, agent: &str) -> Follower {
        Follower {
            user: self.user.clone(),
            chat: self.chat,
            node: node.to_owned(),
            agent: agent.to_owned(),
        }
    }
}

/// The commands each agent takes, for `help`.
pub(super) fn usage(node: &str, agent: &str) -> String {
    let mut usage = format!(
        "{node} {agent} start [--cwd DIR] [--wait SECS] \"PROMPT\"\n\
         {node} {agent} status [ID] [--wait SECS]\n\
         {node} {agent} prompt ID [--wait SECS] \"TEXT\"\n\
         {node} {agent} answer ID [--wait SECS] OPTION_OR_TEXT\n\
         {node} {agent} cancel ID\n\
         {node} {agent} close ID\n\
         {node} {agent} transcript ID [--last N]\n\
         {node} {agent} sessions [--all]\n"
    );
    if agent == EXTERNAL_AGENT {
        usage.push_str(&format!(
            "{node} {agent} follow ID\n\
             {node} {agent} unfollow ID\n\
             {node} {agent} takeover ID [--wait SECS] [\"PROMPT\"]\n"
        ));
    }
    usage
}

/// Runs one agent command; the text is the result, the error a failure.
pub(super) async fn run(
    online: &Online,
    node: &str,
    agent: &str,
    words: &[&str],
    asker: &Asker<'_>,
) -> Result<String, String> {
    let Some((command, rest)) = words.split_first() else {
        return Err(format!("Which command?\n{}", usage(node, agent)));
    };
    let call = Call {
        online,
        node,
        agent,
    };
    match *command {
        "start" => {
            let args = parse(rest, &["--cwd", "--wait"])?;
            let prompt = args.words.join(" ");
            if prompt.trim().is_empty() {
                return Err("start needs a prompt.".into());
            }
            let wait = args.secs("--wait", DEFAULT_WAIT_SECS)?;
            let cwd = args.flag("--cwd").map(str::to_owned);
            let link = online
                .start_agent(agent, cwd, prompt)
                .await
                .map_err(|e| format!("Could not start {agent}: {e:#}"))?;
            let now = link.now();
            online.report_to(&now.info.id, asker.follower(node, agent));
            let mark = now.mark();
            Ok(call.report(&link.wait(mark, wait).await, Some(wait)))
        }
        "status" => {
            let args = parse(rest, &["--wait"])?;
            match &args.words[..] {
                [] => call.list().await,
                [id] => {
                    let link = call.attach(id).await?;
                    let now = link.now();
                    let waited = match args.flag("--wait") {
                        Some(_) if !now.settled() => {
                            let wait = args.secs("--wait", DEFAULT_WAIT_SECS)?;
                            Some((link.wait(now.mark(), wait).await, wait))
                        }
                        _ => None,
                    };
                    Ok(match waited {
                        Some((watched, wait)) => call.report(&watched, Some(wait)),
                        None => call.report(&now, None),
                    })
                }
                _ => Err(format!(
                    "status takes one session id.\n{}",
                    usage(node, agent)
                )),
            }
        }
        "prompt" | "answer" => {
            let args = parse(rest, &["--wait"])?;
            let Some((id, text)) = args.words.split_first() else {
                return Err(format!("{command} needs a session id and text."));
            };
            let text = text.join(" ");
            if text.trim().is_empty() {
                return Err(format!("{command} needs text after the session id."));
            }
            let wait = args.secs("--wait", DEFAULT_WAIT_SECS)?;
            let link = call.attach(id).await?;
            let before = link.now();
            let request = if *command == "prompt" {
                Request::Prompt { text }
            } else {
                let Some(question) = &before.info.question else {
                    return Err(format!(
                        "Session {id} has no open question.\n{}",
                        call.report(&before, None)
                    ));
                };
                Request::Answer {
                    question: question.id,
                    reply: text,
                }
            };
            online.report_to(&before.info.id, asker.follower(node, agent));
            link.send(request)
                .await
                .map_err(|e| format!("The node did not take it: {e:#}"))?;
            Ok(call.report(&link.wait(before.mark(), wait).await, Some(wait)))
        }
        "cancel" | "close" => {
            let [id] = rest else {
                return Err(format!("{command} takes one session id."));
            };
            let link = call.attach(id).await?;
            let before = link.now();
            let request = if *command == "cancel" {
                Request::Cancel {}
            } else {
                Request::Close {}
            };
            link.send(request)
                .await
                .map_err(|e| format!("The node did not take it: {e:#}"))?;
            let after = if before.settled() && *command == "cancel" {
                before
            } else {
                link.wait(before.mark(), STOP_WAIT).await
            };
            Ok(call.report(&after, None))
        }
        "transcript" => {
            let args = parse(rest, &["--last"])?;
            let [id] = &args.words[..] else {
                return Err("transcript takes one session id.".into());
            };
            let last = match args.flag("--last") {
                Some(value) => value
                    .parse()
                    .map_err(|_| format!("--last takes a number, not {value:?}."))?,
                None => DEFAULT_LAST,
            };
            let (entries, earlier) = online
                .transcript(id, last)
                .await
                .map_err(|e| format!("Could not read the transcript: {e:#}"))?;
            let mut out = String::new();
            if earlier > 0 {
                let _ = writeln!(out, "({earlier} earlier entries)");
            }
            for entry in entries {
                let text = cut(&entry.text, MAX_ENTRY_SHOWN);
                let _ = writeln!(out, "[{}] {text}", entry.kind.as_str());
                if entry.kind == EntryKind::Agent {
                    out.push('\n');
                }
            }
            if out.is_empty() {
                out.push_str("The transcript is empty.");
            }
            Ok(out)
        }
        "sessions" => match rest {
            [] => call.list().await,
            ["--all"] => {
                let mut out = call.list().await?;
                if agent == EXTERNAL_AGENT {
                    out.push_str("\n\n");
                    out.push_str(&call.external().await?);
                }
                Ok(out)
            }
            _ => Err(format!(
                "sessions takes only --all.\n{}",
                usage(node, agent)
            )),
        },
        "follow" | "unfollow" | "takeover" if agent != EXTERNAL_AGENT => Err(format!(
            "Only Claude Code sessions started outside Beep can be followed or taken over, \
             not {agent}."
        )),
        "follow" => {
            let [id] = rest else {
                return Err("follow takes one session id.".into());
            };
            call.follow(id, asker).await
        }
        "unfollow" => {
            let [id] = rest else {
                return Err("unfollow takes one session id.".into());
            };
            let nodes = &asker.app.nodes;
            Ok(if nodes.follows.unfollow(asker.app, asker.endpoint, id) {
                format!("Stopped following {id} on {node}.")
            } else {
                format!("{id} was not followed on {node}.")
            })
        }
        "takeover" => {
            let args = parse(rest, &["--wait"])?;
            let Some((id, prompt)) = args.words.split_first() else {
                return Err("takeover needs a session id.".into());
            };
            let prompt = prompt.join(" ");
            let prompt = (!prompt.trim().is_empty()).then_some(prompt);
            let wait = args.secs("--wait", DEFAULT_WAIT_SECS)?;
            let prompted = prompt.is_some();
            let link = online
                .take_over(agent, id, prompt)
                .await
                .map_err(|e| format!("Could not take over {id}: {e:#}"))?;
            // It runs here now; the node session reports on it.
            asker
                .app
                .nodes
                .follows
                .unfollow(asker.app, asker.endpoint, id);
            let now = link.now();
            online.report_to(&now.info.id, asker.follower(node, agent));
            let shown = if prompted {
                call.report(&link.wait(now.mark(), wait).await, Some(wait))
            } else {
                call.report(&now, None)
            };
            Ok(format!(
                "Took over {id}: it now runs on {node} as session {}, so steer it with \
                 `{node} {agent} prompt {} \"TEXT\"`.\n{shown}",
                now.info.id, now.info.id
            ))
        }
        other => Err(format!(
            "{agent} has no command {other}.\n{}",
            usage(node, agent)
        )),
    }
}

/// Sessions started outside Beep for the model, newest first.
fn external_list(node: &str, sessions: &[ExternalSession], now: u64) -> String {
    if sessions.is_empty() {
        return format!("No Claude Code sessions started outside Beep on {node}.");
    }
    let mut out = format!("Claude Code sessions started outside Beep on {node}, newest first:\n");
    for session in sessions {
        out.push_str(&external_line(session, now));
    }
    out.trim_end().to_owned()
}

fn external_line(session: &ExternalSession, now: u64) -> String {
    let mut out = format!("- {} in {}: {}", session.id, session.cwd, session.presence);
    if let Some(pid) = session.pid {
        let _ = write!(out, " (pid {pid})");
    }
    let _ = write!(
        out,
        ", last active {}",
        ago(now.saturating_sub(session.last_activity))
    );
    if let Some(title) = &session.title {
        let _ = write!(out, ", \"{title}\"");
    }
    if let Some(node_session) = &session.taken_over {
        let _ = write!(out, ", runs here as session {node_session}");
    }
    out.push('\n');
    if !session.last_prompt.is_empty() {
        let _ = writeln!(out, "  Last prompt: {}", session.last_prompt);
    }
    if !session.last_message.is_empty() {
        let _ = writeln!(out, "  Last message: {}", session.last_message);
    }
    out
}

fn ago(secs: u64) -> String {
    match secs / 60 {
        0 => "just now".into(),
        minutes @ 1..60 => format!("{minutes} min ago"),
        minutes @ 60..1440 => format!("{} h ago", minutes / 60),
        minutes => format!("{} days ago", minutes / 1440),
    }
}

fn unix_now() -> u64 {
    u64::try_from(Utc::now().timestamp()).unwrap_or(0)
}

/// `<node> sessions`: every agent session on the node.
pub(super) async fn sessions(online: &Online, node: &str) -> Result<String, String> {
    let sessions = online
        .agent_sessions()
        .await
        .map_err(|e| format!("Could not list the sessions: {e:#}"))?;
    Ok(list(node, &sessions))
}

fn list(node: &str, sessions: &[SessionInfo]) -> String {
    if sessions.is_empty() {
        return format!("No agent sessions on {node}.");
    }
    let mut out = String::new();
    for info in sessions {
        let _ = write!(
            out,
            "- {} {} in {}: {}, {} turns",
            info.id, info.agent, info.cwd, info.state.status, info.turns
        );
        if let Some(title) = &info.title {
            let _ = write!(out, ", \"{title}\"");
        }
        out.push('\n');
    }
    out
}

struct Call<'a> {
    online: &'a Online,
    node: &'a str,
    agent: &'a str,
}

impl Call<'_> {
    async fn attach(&self, id: &str) -> Result<std::sync::Arc<crate::nodes::AgentLink>, String> {
        self.online
            .agent_session(self.agent, id)
            .await
            .map_err(|e| format!("Could not reach session {id}: {e:#}"))
    }

    async fn list(&self) -> Result<String, String> {
        let mut sessions = self
            .online
            .agent_sessions()
            .await
            .map_err(|e| format!("Could not list the sessions: {e:#}"))?;
        sessions.retain(|info| info.agent == self.agent);
        Ok(list(self.node, &sessions))
    }

    async fn external(&self) -> Result<String, String> {
        let sessions = self
            .online
            .external_sessions(self.agent, EXTERNAL_LISTED, None)
            .await
            .map_err(|e| format!("Could not list the sessions started outside Beep: {e:#}"))?;
        Ok(external_list(self.node, &sessions, unix_now()))
    }

    async fn follow(&self, id: &str, asker: &Asker<'_>) -> Result<String, String> {
        let found = self
            .online
            .external_sessions(self.agent, 1, Some(id.to_owned()))
            .await
            .map_err(|e| format!("Could not look up {id}: {e:#}"))?;
        let Some(session) = found.first() else {
            return Err(format!(
                "No Claude Code session {id} on {}. `{} {} sessions --all` lists them.",
                self.node, self.node, self.agent
            ));
        };
        if let Some(node_session) = &session.taken_over {
            return Err(format!(
                "{id} runs here as session {node_session}; `{} {} status {node_session}` \
                 checks it.",
                self.node, self.agent
            ));
        }
        let follower = asker.follower(self.node, self.agent);
        asker
            .app
            .nodes
            .follows
            .follow(asker.app, asker.endpoint, id, follower)
            .map_err(|e| format!("Could not follow {id}: {e:#}"))?;
        let live = if session.presence.is_live() {
            "It is open in the user's terminal or IDE, so Beep can only follow it; only the \
             user can answer it there."
        } else {
            "Nothing runs it now; `takeover` would resume it here."
        };
        Ok(format!(
            "Following {id} on {}. This chat hears when it finishes a turn; when it waits \
             for permission too if beep-node's hooks are installed there \
             (`beep-node hooks install`). {live}\n{}",
            self.node,
            external_line(session, unix_now()).trim_end()
        ))
    }

    fn report(&self, watched: &Watched, waited: Option<Duration>) -> String {
        describe(self.node, self.agent, watched, waited)
    }
}

/// A session's state for the model. `waited` is how long the call waited for
/// the turn, if it did.
fn describe(node: &str, agent: &str, watched: &Watched, waited: Option<Duration>) -> String {
    let info = &watched.info;
    let state = &info.state;
    let id = &info.id;
    let status = match (&watched.ended, watched.lost) {
        (Some(reason), _) => format!("ended ({reason})"),
        (None, true) => "lost track of it; it may still run, check its status".to_owned(),
        (None, false) => state.status.to_string(),
    };
    let mut out = format!(
        "{agent} session {id} in {}: {status}, {} turns",
        info.cwd, info.turns
    );
    if state.queued > 0 {
        let _ = write!(out, ", {} prompts waiting", state.queued);
    }
    out.push('\n');
    if let Some(title) = &info.title {
        let _ = writeln!(out, "Title: {title}");
    }
    if state.status == Status::Working && !state.summary.is_empty() {
        let _ = writeln!(out, "Doing: {}", state.summary);
    }
    if !state.plan.is_empty() {
        out.push_str("Plan:\n");
        for entry in &state.plan {
            let mark = match entry.status {
                PlanStatus::Completed => "done",
                PlanStatus::InProgress => "now",
                _ => "todo",
            };
            let _ = writeln!(out, "  [{mark}] {}", entry.content);
        }
    }
    if !state.files_touched.is_empty() {
        let _ = writeln!(out, "Files changed: {}", state.files_touched.join(", "));
    }
    if let Some(error) = &state.error
        && watched.ended.as_ref() != Some(error)
    {
        let _ = writeln!(out, "Error: {error}");
    }
    if let Some(question) = &info.question {
        let _ = writeln!(out, "Question {}: {}", question.id, question.text);
        if let Some(detail) = &question.detail {
            let _ = writeln!(out, "  {detail}");
        }
        let options: Vec<String> = question
            .options
            .iter()
            .enumerate()
            .map(|(n, option)| format!("{}. {} ({})", n + 1, option.name, option.id))
            .collect();
        let _ = writeln!(out, "Options: {}", options.join(", "));
        let _ = writeln!(
            out,
            "Answer with `{node} {agent} answer {id} OPTION`. Any other text declines \
             and goes to the agent as its next prompt."
        );
    }
    if info.turns > 0 && state.status != Status::Working && !info.last_message.is_empty() {
        let stop = watched
            .last_stop
            .map_or("ended".to_owned(), |stop| stop.to_string());
        let _ = writeln!(
            out,
            "Last turn {stop}. Its last message:\n{}",
            info.last_message
        );
    }
    if let Some(waited) = waited
        && matches!(state.status, Status::Working | Status::Starting)
        && watched.ended.is_none()
        && !watched.lost
    {
        let _ = writeln!(
            out,
            "Still working after {}s. Check later with `{node} {agent} status {id} --wait 120`.",
            waited.as_secs()
        );
    }
    out.trim_end().to_owned()
}

/// Options and positional words. Options go anywhere; once a positional word
/// came, an unknown `--word` is text.
struct Args<'a> {
    flags: Vec<(&'a str, &'a str)>,
    words: Vec<&'a str>,
}

fn parse<'a>(words: &[&'a str], allowed: &[&str]) -> Result<Args<'a>, String> {
    let mut args = Args {
        flags: Vec::new(),
        words: Vec::new(),
    };
    let mut rest = words.iter().copied();
    while let Some(word) = rest.next() {
        if word == "--" {
            args.words.extend(rest.by_ref());
            break;
        }
        let Some(stripped) = word.strip_prefix("--") else {
            args.words.push(word);
            continue;
        };
        let (name, inline) = match stripped.split_once('=') {
            Some((name, value)) => (&word[..name.len() + 2], Some(value)),
            None => (word, None),
        };
        if !allowed.contains(&name) {
            if args.words.is_empty() {
                return Err(format!("Unknown option {name}."));
            }
            args.words.push(word);
            continue;
        }
        let value = match inline {
            Some(value) => value,
            None => rest
                .next()
                .ok_or_else(|| format!("{name} needs a value."))?,
        };
        args.flags.push((name, value));
    }
    Ok(args)
}

impl Args<'_> {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(flag, _)| *flag == name)
            .map(|(_, value)| *value)
    }

    fn secs(&self, name: &str, default: u64) -> Result<Duration, String> {
        let secs = match self.flag(name) {
            Some(value) => value
                .parse::<u64>()
                .map_err(|_| format!("{name} takes seconds, not {value:?}."))?,
            None => default,
        };
        Ok(Duration::from_secs(secs.min(MAX_WAIT_SECS)))
    }
}

fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [...]", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::node::agent::{AnswerOption, OptionKind, PlanEntry, Question, State};

    #[test]
    fn options_go_anywhere_and_unknown_ones_after_text_are_text() {
        let args = parse(
            &[
                "--cwd",
                "/srv/project",
                "fix",
                "--wait=30",
                "the",
                "--fast",
                "tests",
            ],
            &["--cwd", "--wait"],
        )
        .unwrap();
        assert_eq!(args.flag("--cwd"), Some("/srv/project"));
        assert_eq!(args.secs("--wait", 5).unwrap(), Duration::from_secs(30));
        assert_eq!(args.words, ["fix", "the", "--fast", "tests"]);
        assert!(parse(&["--fast", "x"], &["--wait"]).is_err());
        assert!(parse(&["x", "--wait"], &["--wait"]).is_err());
        let args = parse(&["--", "--wait", "5"], &["--wait"]).unwrap();
        assert_eq!(args.words, ["--wait", "5"]);
        let args = parse(&["--wait", "99999"], &["--wait"]).unwrap();
        assert_eq!(
            args.secs("--wait", 5).unwrap(),
            Duration::from_secs(MAX_WAIT_SECS)
        );
    }

    #[test]
    fn a_report_shows_the_question_and_how_to_answer() {
        let watched = Watched {
            info: SessionInfo {
                id: "a3f9c1".into(),
                agent: "claude".into(),
                cwd: "/srv/project".into(),
                turns: 0,
                state: State {
                    status: Status::Asking,
                    plan: vec![PlanEntry {
                        content: "Fix the test".into(),
                        status: PlanStatus::InProgress,
                    }],
                    files_touched: vec!["src/lib.rs".into()],
                    ..State::default()
                },
                question: Some(Question {
                    id: 2,
                    text: "Run cargo test".into(),
                    detail: Some("cargo test -p core".into()),
                    options: vec![
                        AnswerOption {
                            id: "allow".into(),
                            name: "Allow".into(),
                            kind: OptionKind::AllowOnce,
                        },
                        AnswerOption {
                            id: "reject".into(),
                            name: "Reject".into(),
                            kind: OptionKind::RejectOnce,
                        },
                    ],
                }),
                ..SessionInfo::default()
            },
            ..Watched::default()
        };
        assert_eq!(
            describe("build-box", "claude", &watched, None),
            "claude session a3f9c1 in /srv/project: waiting for an answer, 0 turns\n\
             Plan:\n  [now] Fix the test\n\
             Files changed: src/lib.rs\n\
             Question 2: Run cargo test\n  cargo test -p core\n\
             Options: 1. Allow (allow), 2. Reject (reject)\n\
             Answer with `build-box claude answer a3f9c1 OPTION`. Any other text declines \
             and goes to the agent as its next prompt."
        );
    }

    #[test]
    fn outside_sessions_show_where_they_run_and_what_they_said() {
        let session = ExternalSession {
            id: "0f3c2a9e-1111-4222-8333-944455556666".into(),
            agent: "claude".into(),
            cwd: "/srv/project".into(),
            title: Some("Fix the flaky test".into()),
            last_activity: 1_000,
            presence: crate::protocol::node::agent::Presence::Waiting,
            pid: Some(41),
            last_prompt: "run the suite".into(),
            last_message: "All tests pass.".into(),
            ..ExternalSession::default()
        };
        assert_eq!(
            external_list("build-box", &[session], 1_000 + 7 * 60),
            "Claude Code sessions started outside Beep on build-box, newest first:\n\
             - 0f3c2a9e-1111-4222-8333-944455556666 in /srv/project: waiting for the user \
             (pid 41), last active 7 min ago, \"Fix the flaky test\"\n  \
             Last prompt: run the suite\n  Last message: All tests pass."
        );
        assert_eq!(
            external_list("build-box", &[], 0),
            "No Claude Code sessions started outside Beep on build-box."
        );
    }

    #[test]
    fn sessions_list_one_per_line() {
        let info = SessionInfo {
            id: "a3f9c1".into(),
            agent: "codex".into(),
            cwd: "/srv/project".into(),
            turns: 3,
            title: Some("Fix tests".into()),
            ..SessionInfo::default()
        };
        assert_eq!(
            list("build-box", &[info]),
            "- a3f9c1 codex in /srv/project: starting, 3 turns, \"Fix tests\"\n"
        );
        assert_eq!(list("build-box", &[]), "No agent sessions on build-box.");
    }
}
