//! One agent session: the adapter process and the task that talks ACP to
//! it. The task owns the process; streams send it orders and follow its
//! state through a watch channel.

use super::{
    AgentKind,
    acp::{self, Incoming},
    live::{Live, cut},
};
use crate::shell::GroupLeader;
use anyhow::{Context, Result};
use beep_proto::node::agent::{
    AnswerOption, EntryKind, OptionKind, Question, SessionInfo, State, Status, StopReason,
};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    process::Stdio,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command as Process},
    sync::{mpsc, oneshot, watch},
};
use tokio_util::task::AbortOnDropHandle;

/// The most prompts waiting behind a running turn.
const MAX_QUEUED: usize = 8;
/// The stderr lines kept to say why an agent stopped.
const STDERR_LINES: usize = 5;
const ORDERS: usize = 16;

/// What a stream asks of a session.
#[derive(Debug)]
pub(super) enum Command {
    Prompt(String),
    Answer { question: u32, reply: String },
    Cancel,
    Close,
}

struct Order {
    command: Command,
    done: oneshot::Sender<Result<(), String>>,
}

pub(super) struct Session {
    pub(super) id: String,
    pub(super) agent: String,
    orders: mpsc::Sender<Order>,
    pub(super) live: watch::Receiver<Live>,
    /// Dropping the session stops its task and with it the agent.
    _task: AbortOnDropHandle<()>,
}

/// What a new session starts with.
pub(super) struct Start<'a> {
    pub(super) id: String,
    pub(super) kind: &'a AgentKind,
    pub(super) cwd: &'a Path,
    pub(super) prompt: Option<String>,
    /// The agent's id of an earlier session to resume instead of a new one.
    pub(super) resume: Option<String>,
}

impl Session {
    pub(super) fn start(start: Start<'_>) -> Result<Self> {
        let Start {
            id,
            kind,
            cwd,
            prompt,
            resume,
        } = start;
        let mut child = Process::new(&kind.program)
            .args(&kind.args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Its own process group, so closing it stops what it started.
            .process_group(0)
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {}", kind.program))?;
        let stdin = child.stdin.take().context("the agent has no stdin")?;
        let cwd = cwd.display().to_string();
        let info = SessionInfo {
            id: id.clone(),
            agent: kind.name.clone(),
            cwd: cwd.clone(),
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
            ..SessionInfo::default()
        };
        let mut first = Live::new(info);
        // Known from the start, so a second takeover of it is refused.
        first.agent_session = resume.clone();
        let (live_tx, live) = watch::channel(first);
        let (orders, orders_rx) = mpsc::channel(ORDERS);
        let actor = Actor {
            stdin,
            live: live_tx,
            cwd,
            next_id: 0,
            pending: HashMap::new(),
            acp_session: None,
            queued: prompt.into_iter().collect(),
            resume,
            skip_permissions: kind.name == "claude",
            asked: VecDeque::new(),
            next_question: 1,
            stderr: VecDeque::new(),
        };
        let task = tokio::spawn(actor.run(child, orders_rx));
        Ok(Self {
            id,
            agent: kind.name.clone(),
            orders,
            live,
            _task: AbortOnDropHandle::new(task),
        })
    }

    pub(super) fn info(&self) -> SessionInfo {
        self.live.borrow().info.clone()
    }

    /// Hands `command` to the session and waits until it is done or refused.
    pub(super) async fn order(&self, command: Command) -> Result<(), String> {
        let (done, answer) = oneshot::channel();
        self.orders
            .send(Order { command, done })
            .await
            .map_err(|_| "the session has ended".to_owned())?;
        answer
            .await
            .map_err(|_| "the session has ended".to_owned())?
    }
}

/// What a request we sent was for.
enum Pending {
    Initialize,
    NewSession,
    /// `session/resume` or `session/load` of this session.
    Resume(String),
    /// `session/set_mode` to the mode that asks no permission.
    Bypass,
    Prompt,
}

/// A permission question waiting for its answer.
struct Asked {
    question: Question,
    rpc: Value,
}

struct Actor {
    stdin: ChildStdin,
    live: watch::Sender<Live>,
    cwd: String,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    /// The agent's id for the session, once it made one.
    acp_session: Option<String>,
    /// The agent's id of an earlier session to resume.
    resume: Option<String>,
    /// The agent runs without permission prompts: Claude Code, as with
    /// `--dangerously-skip-permissions`. A permission request that comes
    /// anyway is allowed rather than asked.
    skip_permissions: bool,
    /// Prompts waiting for the agent to be free, the first prompt among them.
    queued: VecDeque<String>,
    /// Questions waiting, the first one shown.
    asked: VecDeque<Asked>,
    next_question: u32,
    /// The last lines the agent wrote to stderr.
    stderr: VecDeque<String>,
}

/// Why the session's task stopped.
enum End {
    Closed,
    Exited,
    Failed(String),
}

impl Actor {
    async fn run(mut self, child: Child, mut orders: mpsc::Receiver<Order>) {
        // Stops the agent's whole process group however the task ends,
        // aborted with its session included.
        let mut child = GroupLeader::new(child);
        let end = self.serve(child.child_mut(), &mut orders).await;
        let closed = matches!(end, Ok(End::Closed));
        let reason = match end {
            Ok(End::Closed) => "closed".to_owned(),
            Ok(End::Exited) => match self.stderr.back() {
                Some(line) => format!("the agent exited: {line}"),
                None => "the agent exited".to_owned(),
            },
            Ok(End::Failed(reason)) => reason,
            Err(e) => format!("lost the agent: {e:#}"),
        };
        log::info!("agent session ended: {reason}");
        drop(child);
        // Orders that came in too late hear the session ended.
        orders.close();
        self.live.send_modify(|live| {
            live.info.question = None;
            live.info.state.status = Status::Ended;
            live.info.state.queued = 0;
            if !closed {
                live.info.state.error = Some(reason);
            }
        });
    }

    async fn serve(
        &mut self,
        child: &mut Child,
        orders: &mut mpsc::Receiver<Order>,
    ) -> Result<End> {
        let stdout = child.stdout.take().context("the agent has no stdout")?;
        let stderr = child.stderr.take().context("the agent has no stderr")?;
        let mut lines = BufReader::new(stdout).lines();
        let mut errors = BufReader::new(stderr).lines();
        let mut errors_open = true;
        let id = self.send_request("initialize", acp::initialize()).await?;
        self.pending.insert(id, Pending::Initialize);
        loop {
            tokio::select! {
                line = lines.next_line() => match line? {
                    Some(line) => {
                        if let Some(end) = self.incoming(&line).await? {
                            return Ok(end);
                        }
                    }
                    None => return Ok(End::Exited),
                },
                line = errors.next_line(), if errors_open => match line {
                    Ok(Some(line)) if !line.trim().is_empty() => {
                        log::debug!("agent stderr: {line}");
                        if self.stderr.len() >= STDERR_LINES {
                            self.stderr.pop_front();
                        }
                        self.stderr.push_back(cut(line.trim(), 300));
                    }
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => errors_open = false,
                },
                order = orders.recv() => {
                    let Some(Order { command, done }) = order else {
                        return Ok(End::Closed);
                    };
                    if matches!(command, Command::Close) {
                        let _ = done.send(Ok(()));
                        return Ok(End::Closed);
                    }
                    let result = self.command(command).await?;
                    // The stream may have gone; the order still happened.
                    let _ = done.send(result);
                }
            }
        }
    }

    async fn incoming(&mut self, line: &str) -> Result<Option<End>> {
        let Some(message) = acp::parse(line) else {
            log::debug!(
                "agent wrote a line that is not JSON-RPC: {}",
                cut(line, 200)
            );
            return Ok(None);
        };
        match message {
            Incoming::Response { id, result } => match (self.pending.remove(&id), result) {
                (Some(Pending::Initialize), Ok(result)) => match self.resume.take() {
                    Some(session) => {
                        let Some(method) = acp::resume_method(&result) else {
                            return Ok(Some(End::Failed(
                                "the agent cannot resume a session".into(),
                            )));
                        };
                        let params = acp::resume_session(&session, &self.cwd);
                        let id = self.send_request(method, params).await?;
                        self.pending.insert(id, Pending::Resume(session));
                    }
                    None => {
                        let id = self
                            .send_request("session/new", acp::new_session(&self.cwd))
                            .await?;
                        self.pending.insert(id, Pending::NewSession);
                    }
                },
                (Some(Pending::NewSession), Ok(result)) => {
                    let Some(session) = result["sessionId"].as_str() else {
                        return Ok(Some(End::Failed("the agent made no session".into())));
                    };
                    self.ready(session.to_owned()).await?;
                }
                (Some(Pending::Resume(session)), Ok(_)) => self.ready(session).await?,
                (Some(Pending::Bypass), result) => {
                    if let Err(e) = result {
                        // Requests that still come are allowed in `ask`.
                        log::warn!("the agent kept asking permission: {e}");
                    }
                    self.idle().await?;
                }
                (Some(Pending::Resume(_)), Err(e)) => {
                    return Ok(Some(End::Failed(format!(
                        "the agent could not resume the session: {e}"
                    ))));
                }
                (Some(Pending::Initialize | Pending::NewSession), Err(e)) => {
                    return Ok(Some(End::Failed(format!("the agent did not start: {e}"))));
                }
                (Some(Pending::Prompt), result) => {
                    let (stop, error) = match result {
                        Ok(result) => (
                            StopReason::from_acp(result["stopReason"].as_str().unwrap_or("")),
                            None,
                        ),
                        Err(e) => (StopReason::Failed, Some(e)),
                    };
                    self.turn_done(stop, error).await?;
                }
                (None, _) => log::debug!("the agent answered unknown request {id}"),
            },
            Incoming::Request { id, method, params } => {
                if method == "session/request_permission" {
                    if !self.allow(&id, &params).await? {
                        self.ask(id, &params);
                    }
                } else {
                    self.write(&acp::not_supported(&id, &method)).await?;
                }
            }
            Incoming::Notification { method, params } => {
                if method == "session/update" {
                    self.live
                        .send_if_modified(|live| live.apply(&params["update"]));
                }
            }
        }
        Ok(None)
    }

    /// The agent made or resumed its session `session`: prompts may go.
    async fn ready(&mut self, session: String) -> Result<()> {
        self.acp_session = Some(session.clone());
        self.live
            .send_modify(|live| live.agent_session = Some(session.clone()));
        if self.skip_permissions {
            // The first prompt waits for the mode, so it never asks.
            let params = acp::set_mode(&session, acp::BYPASS_MODE);
            let id = self.send_request("session/set_mode", params).await?;
            self.pending.insert(id, Pending::Bypass);
            return Ok(());
        }
        self.idle().await
    }

    /// The session takes prompts: sends the first one waiting.
    async fn idle(&mut self) -> Result<()> {
        self.live
            .send_modify(|live| live.info.state.status = Status::Idle);
        self.next_prompt().await
    }

    async fn command(&mut self, command: Command) -> Result<Result<(), String>> {
        match command {
            Command::Prompt(text) => {
                if self.queued.len() >= MAX_QUEUED {
                    return Ok(Err(format!(
                        "{MAX_QUEUED} prompts already wait for this turn"
                    )));
                }
                self.queued.push_back(text);
                self.show_queued();
                if self.live.borrow().status() == Status::Idle {
                    self.next_prompt().await?;
                }
                Ok(Ok(()))
            }
            Command::Answer { question, reply } => self.answer(question, &reply).await,
            Command::Cancel => {
                self.queued.clear();
                self.show_queued();
                while let Some(asked) = self.asked.pop_front() {
                    self.write(&acp::response(&asked.rpc, acp::cancelled()))
                        .await?;
                }
                if let Some(session) = &self.acp_session
                    && self.live.borrow().status() != Status::Idle
                {
                    let cancel = acp::notification("session/cancel", acp::cancel(session));
                    self.write(&cancel).await?;
                }
                self.show_question();
                Ok(Ok(()))
            }
            Command::Close => Ok(Ok(())),
        }
    }

    /// Allows a permission request in a session that asks none; `false`
    /// when it should be asked after all: the session asks, or the request
    /// offers no way to allow.
    async fn allow(&mut self, rpc: &Value, params: &Value) -> Result<bool> {
        if !self.skip_permissions {
            return Ok(false);
        }
        let question = self.live.borrow().question(0, params);
        let options = &question.options;
        let Some(option) = options
            .iter()
            .find(|option| option.kind == OptionKind::AllowOnce)
            .or_else(|| options.iter().find(|o| o.kind == OptionKind::AllowAlways))
        else {
            return Ok(false);
        };
        self.write(&acp::response(rpc, acp::selected(&option.id)))
            .await?;
        self.live.send_modify(|live| {
            live.push(EntryKind::Question, &format!("Allowed: {}", question.text));
        });
        Ok(true)
    }

    fn ask(&mut self, rpc: Value, params: &Value) {
        let question = self.live.borrow().question(self.next_question, params);
        self.next_question += 1;
        self.live.send_modify(|live| {
            let detail = question
                .detail
                .as_deref()
                .map(|detail| format!(" ({detail})"))
                .unwrap_or_default();
            live.push(
                EntryKind::Question,
                &format!("Asked: {}{detail}", question.text),
            );
        });
        self.asked.push_back(Asked { question, rpc });
        self.show_question();
    }

    async fn answer(&mut self, question: u32, reply: &str) -> Result<Result<(), String>> {
        let Some(asked) = self.asked.front() else {
            return Ok(Err("there is no open question".into()));
        };
        if asked.question.id != question {
            return Ok(Err(format!(
                "question {question} is not open; question {} is",
                asked.question.id
            )));
        }
        let options = &asked.question.options;
        let (chosen, follow_up) = match choose(options, reply) {
            Some(option) => (Some(option), None),
            // Anything else declines and goes to the agent as its next prompt.
            None => (
                options
                    .iter()
                    .find(|option| option.kind == OptionKind::RejectOnce)
                    .or_else(|| options.iter().find(|option| option.kind.rejects())),
                Some(reply.to_owned()),
            ),
        };
        let answer = match chosen {
            Some(option) => acp::selected(&option.id),
            None => acp::cancelled(),
        };
        let name = chosen
            .map_or("declined", |option| option.name.as_str())
            .to_owned();
        let rpc = asked.rpc.clone();
        self.write(&acp::response(&rpc, answer)).await?;
        self.asked.pop_front();
        if let Some(text) = follow_up {
            self.queued.push_front(text);
            self.show_queued();
        }
        self.live.send_modify(|live| {
            live.push(EntryKind::Question, &format!("Answered: {name}"));
        });
        self.show_question();
        Ok(Ok(()))
    }

    /// Shows the first waiting question, or none, in the state.
    fn show_question(&mut self) {
        let question = self.asked.front().map(|asked| asked.question.clone());
        self.live.send_if_modified(|live| {
            let status = match (&question, live.status()) {
                (Some(_), _) => Status::Asking,
                (None, Status::Asking) => Status::Working,
                (None, status) => status,
            };
            let changed = live.info.question != question || live.info.state.status != status;
            live.info.question = question;
            live.info.state.status = status;
            changed
        });
    }

    fn show_queued(&mut self) {
        let queued = u32::try_from(self.queued.len()).unwrap_or(u32::MAX);
        self.live.send_if_modified(|live| {
            let changed = live.info.state.queued != queued;
            live.info.state.queued = queued;
            changed
        });
    }

    async fn turn_done(&mut self, stop: StopReason, error: Option<String>) -> Result<()> {
        // A turn that ends leaves no question open.
        self.asked.clear();
        self.live.send_modify(|live| {
            live.info.turns += 1;
            live.last_stop = Some(stop);
            live.info.question = None;
            let state: &mut State = &mut live.info.state;
            state.status = Status::Idle;
            state.summary.clear();
            state.error = error;
        });
        self.next_prompt().await
    }

    /// Sends the next waiting prompt, if any.
    async fn next_prompt(&mut self) -> Result<()> {
        let Some(session) = self.acp_session.clone() else {
            return Ok(());
        };
        let Some(text) = self.queued.pop_front() else {
            return Ok(());
        };
        let id = self
            .send_request("session/prompt", acp::prompt(&session, &text))
            .await?;
        self.pending.insert(id, Pending::Prompt);
        let queued = u32::try_from(self.queued.len()).unwrap_or(u32::MAX);
        self.live.send_modify(|live| {
            live.prompted(&text);
            live.info.state.queued = queued;
        });
        Ok(())
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&acp::request(id, method, params)).await?;
        Ok(id)
    }

    async fn write(&mut self, line: &str) -> Result<()> {
        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("writing to the agent")?;
        self.stdin.flush().await?;
        Ok(())
    }
}

/// The option `reply` names: by id, number, name or kind, or plain yes/no.
pub(super) fn choose<'a>(options: &'a [AnswerOption], reply: &str) -> Option<&'a AnswerOption> {
    let reply = reply.trim().to_lowercase();
    let by_kind = |kind: OptionKind| options.iter().find(|option| option.kind == kind);
    options
        .iter()
        .find(|option| option.id.to_lowercase() == reply)
        .or_else(|| {
            let n: usize = reply.parse().ok()?;
            options.get(n.checked_sub(1)?)
        })
        .or_else(|| {
            options
                .iter()
                .find(|option| option.name.to_lowercase() == reply)
        })
        .or_else(|| options.iter().find(|option| option.kind.as_str() == reply))
        .or_else(|| match reply.as_str() {
            "allow" | "yes" | "y" | "approve" | "ok" => by_kind(OptionKind::AllowOnce),
            "always" => by_kind(OptionKind::AllowAlways),
            "reject" | "deny" | "no" | "n" => by_kind(OptionKind::RejectOnce),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> Vec<AnswerOption> {
        let option = |id: &str, name: &str, kind| AnswerOption {
            id: id.into(),
            name: name.into(),
            kind,
        };
        vec![
            option("allow", "Allow", OptionKind::AllowOnce),
            option("always", "Always Allow", OptionKind::AllowAlways),
            option("reject", "Reject", OptionKind::RejectOnce),
        ]
    }

    /// An ACP adapter in a few lines of shell: it can resume sessions and
    /// appends each line it reads to the file in `$1`. On the prompt it asks
    /// permission once, then ends the turn.
    const FAKE_ADAPTER: &str = r#"
log() { printf '%s\n' "$line" >> "$1"; }
read line; log "$1"
echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true,"sessionCapabilities":{"resume":{}}}}}'
read line; log "$1"
echo '{"jsonrpc":"2.0","id":1,"result":{}}'
read line; log "$1"
echo '{"jsonrpc":"2.0","id":2,"result":{}}'
read line; log "$1"
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}'
read line; log "$1"
echo '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Picked it up."}}}}'
echo '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
cat > /dev/null
"#;

    /// An adapter that asks permission on its first prompt and waits.
    const ASKING_ADAPTER: &str = r#"
log() { printf '%s\n' "$line" >> "$1"; }
read line; log "$1"
echo '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentCapabilities":{}}}'
read line; log "$1"
echo '{"jsonrpc":"2.0","id":1,"result":{"sessionId":"s"}}'
read line; log "$1"
echo '{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"sessionId":"s","toolCall":{"toolCallId":"t1","title":"Run cargo test"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}}'
cat > /dev/null
"#;

    fn fake(name: &str, log: &Path) -> AgentKind {
        script(name, FAKE_ADAPTER, log)
    }

    fn script(name: &str, script: &str, log: &Path) -> AgentKind {
        AgentKind {
            name: name.into(),
            program: "sh".into(),
            args: vec![
                "-c".into(),
                script.into(),
                "fake-adapter".into(),
                log.display().to_string(),
            ],
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("beep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn settle(session: &Session) {
        let mut live = session.live.clone();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            live.wait_for(|live| {
                live.info.turns >= 1 || matches!(live.status(), Status::Ended | Status::Asking)
            }),
        )
        .await
        .expect("the session settles")
        .unwrap();
    }

    #[tokio::test]
    async fn a_takeover_resumes_without_permission_prompts() {
        let dir = temp("takeover");
        let log = dir.join("sent.jsonl");
        let id = "0f3c2a9e-1111-4222-8333-944455556666";
        let session = Session::start(Start {
            id: "a3f9c1".into(),
            kind: &fake("claude", &log),
            cwd: &dir,
            prompt: Some("carry on".into()),
            resume: Some(id.into()),
        })
        .unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.turns, 1, "{:?}", info.state.error);
        assert_eq!(info.last_message, "Picked it up.");
        assert!(info.question.is_none());
        assert_eq!(session.live.borrow().agent_session.as_deref(), Some(id));
        let sent: Vec<Value> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(sent[1]["method"], "session/resume");
        assert_eq!(sent[1]["params"]["sessionId"], id);
        assert_eq!(sent[1]["params"]["cwd"], dir.display().to_string());
        assert_eq!(sent[2]["method"], "session/set_mode");
        assert_eq!(sent[2]["params"]["modeId"], "bypassPermissions");
        assert_eq!(
            sent[3]["method"], "session/prompt",
            "the prompt waits for the mode"
        );
        // The stray permission request was allowed, not asked.
        assert_eq!(sent[4]["id"], "p1");
        assert_eq!(sent[4]["result"]["outcome"]["optionId"], "allow");
        let (entries, _) = session.live.borrow().transcript(10);
        assert!(
            entries
                .iter()
                .any(|entry| entry.text == "Allowed: Run cargo test")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn other_agents_still_ask() {
        let dir = temp("asks");
        let log = dir.join("sent.jsonl");
        let session = Session::start(Start {
            id: "b4e0d2".into(),
            kind: &script("codex", ASKING_ADAPTER, &log),
            cwd: &dir,
            prompt: Some("go".into()),
            resume: None,
        })
        .unwrap();
        settle(&session).await;
        let info = session.info();
        assert_eq!(info.state.status, Status::Asking);
        assert_eq!(
            info.question.map(|q| q.text).as_deref(),
            Some("Run cargo test")
        );
        let sent = std::fs::read_to_string(&log).unwrap();
        assert!(!sent.contains("session/set_mode"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_reply_picks_an_option_by_id_number_name_or_word() {
        let options = options();
        let pick = |reply| choose(&options, reply).map(|option| option.id.as_str());
        assert_eq!(pick("reject"), Some("reject"));
        assert_eq!(pick("2"), Some("always"));
        assert_eq!(pick("Always Allow"), Some("always"));
        assert_eq!(pick("allow_once"), Some("allow"));
        assert_eq!(pick("yes"), Some("allow"));
        assert_eq!(pick("no"), Some("reject"));
        assert_eq!(pick("4"), None);
        assert_eq!(pick("use the other folder instead"), None);
    }
}
