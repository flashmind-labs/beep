//! The agent's machine tools answered between the machines of a mesh, with
//! no server to ask: the node lists the mesh's devices itself and sends
//! each call to the device it names, over `tools/1`. Calls for a machine the
//! mesh does not hold go to the server when there is one. Between a user's
//! own machines there are no rules to check first.

use super::{Rule, permission};
use crate::{
    files::transfer::{Incoming, Transfers},
    link::{
        Offers,
        peer_tools::{self, Caller},
    },
    machine::env,
};
use anyhow::{Context, Result, bail};
use beep_mesh::{DeviceKind, Key, Mesh};
use beep_proto::node::{
    EnvReport, LinkRx, LinkTx, Open, Opened, link,
    mesh_state::{StateValue, SyncSpec},
    status::{self, Report},
    tools::{Approved, PROTOCOL, REFUSED, Reply, Request, Tool},
    transfer::Reply as TransferReply,
};
use iroh::EndpointId;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fmt::Write, time::Duration};
use tokio::time::timeout;

/// How long a call waits for the link to a machine the mesh holds but is
/// not connected to yet.
const LINK_WAIT: Duration = Duration::from_secs(15);
/// How many times opening a call's stream is tried.
const HANDSHAKES: u32 = 3;
/// How long a machine has to report itself for `list_machines`.
const STATUS_WAIT: Duration = Duration::from_secs(5);
/// How long the machine has to take a call's stream.
const OPEN_LIMIT: Duration = Duration::from_secs(15);
/// How long one call may run: a command runs at most half an hour.
const CALL_LIMIT: Duration = Duration::from_secs(35 * 60);

/// A machine as the agent names it: lower case, one word.
pub(super) fn command_name(name: &str) -> String {
    name.trim().to_lowercase().replace(' ', "-")
}

/// One device of the mesh.
pub(super) struct Machine {
    pub(super) id: EndpointId,
    pub(super) name: String,
}

pub(super) fn machines(mesh: &Mesh) -> Vec<Machine> {
    mesh.devices()
        .into_iter()
        .map(|status| Machine {
            id: status.device.id,
            name: command_name(&status.device.name),
        })
        .collect()
}

fn done(text: impl Into<String>, failed: bool) -> Reply {
    Reply::Done {
        text: text.into(),
        failed,
    }
}

/// Where a call goes, from what the node has of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Route {
    /// To the mesh.
    Direct,
    /// To the server, which holds the user's rules and asks first.
    Server,
}

/// The server holds the rules for a send, a run and a log read. With its link
/// up those go through it. With the link down, or no server, they go direct,
/// and `asks_while_down` says whether the user is asked first.
pub(super) fn route(tool: Tool, has_server: bool, link_up: bool) -> Route {
    match tool {
        Tool::RunOn | Tool::SendFiles | Tool::MachineLog if has_server && link_up => Route::Server,
        _ => Route::Direct,
    }
}

/// The rule for a call made while the server cannot be asked: the node has a server, its link is
/// down, and the last rule heard for the tool decides. A tool never heard of asks only for a send.
/// With no server ever, nothing asks.
fn rule_while_down(offers: &Offers, tool: Tool) -> Rule {
    if !offers.tools.has_server()
        || offers.tools.connected()
        || !matches!(tool, Tool::RunOn | Tool::SendFiles | Tool::MachineLog)
    {
        return Rule::Run;
    }
    offers
        .tools
        .heard(tool)
        .unwrap_or(if tool == Tool::SendFiles {
            Rule::Ask
        } else {
            Rule::Run
        })
}

/// The reply for `request`, or `None` when the server answers it: a tool
/// that is not between machines, or a machine the mesh does not hold.
pub(super) async fn call(offers: &Offers, mesh: &Mesh, request: &Request) -> Option<Reply> {
    let args: Value = serde_json::from_str(&request.args).unwrap_or(Value::Null);
    let machine = |args: &Value| {
        args.get("machine")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match request.tool {
        // The server's list has more to say, so it leads when there is one.
        Tool::ListMachines if offers.tools.connected() => {
            Some(merged_list(offers, mesh, request).await)
        }
        Tool::ListMachines => Some(list(mesh).await),
        Tool::RunOn | Tool::MachineLog | Tool::SendFiles => {
            let name = machine(&args)?;
            let Some(target) = machines(mesh)
                .into_iter()
                .find(|m| m.name == command_name(&name))
            else {
                return missing(offers, mesh, &name);
            };
            let rule = rule_while_down(offers, request.tool);
            if let Some(stopped) = permission::enforce(offers, &rule, request, &args).await {
                return Some(stopped);
            }
            Some(to_target(offers, mesh, &target, request, &args).await)
        }
        Tool::SyncProject => sync_project(offers, mesh, request, &args).await,
        Tool::JobStatus | Tool::Cancel => {
            let id = args.get("id")?.as_str()?;
            let job = Job::parse(id)?;
            let target = machines(mesh).into_iter().find(|m| m.name == job.machine)?;
            let sent = Request {
                args: json!({ "id": job.id.to_string() }).to_string(),
                ..request.clone()
            };
            Some(to_machine(offers, mesh, &target, &sent).await)
        }
        _ => None,
    }
}

/// Sends `request` to `target`, which the mesh holds.
async fn to_target(
    offers: &Offers,
    mesh: &Mesh,
    target: &Machine,
    request: &Request,
    args: &Value,
) -> Reply {
    if request.tool == Tool::SendFiles {
        return send_files(offers, mesh, target, request, args).await;
    }
    to_machine(offers, mesh, target, request).await
}

/// The reply for a call the server would otherwise answer, when its target
/// is a machine only the mesh knows (one that joined by ticket): the server
/// has no such machine, so the call goes direct. `None` when the server
/// knows the target, which keeps its rules, or the mesh does not hold it.
pub(super) async fn mesh_only(offers: &Offers, mesh: &Mesh, request: &Request) -> Option<Reply> {
    if !matches!(
        request.tool,
        Tool::RunOn | Tool::MachineLog | Tool::SendFiles
    ) {
        return None;
    }
    let args: Value = serde_json::from_str(&request.args).ok()?;
    let name = command_name(args.get("machine")?.as_str()?);
    let target = machines(mesh).into_iter().find(|m| m.name == name)?;
    // A send to a machine of the mesh always goes direct: through the server it would need
    // shared folders.
    if request.tool != Tool::SendFiles {
        let listing = Request {
            tool: Tool::ListMachines,
            args: "{}".to_owned(),
            ..request.clone()
        };
        // An answer that is not a list leaves the call to the server, which says why.
        let Ok(Reply::Done {
            text,
            failed: false,
        }) = offers.tools.call(&listing).await
        else {
            return None;
        };
        if listed(&text).contains(&name) {
            return None;
        }
    }
    // The server cannot show a card for a machine it lacks, so when its rule asks, the node
    // asks the user itself, and a rule that refuses still refuses. If the server cannot be
    // reached to ask, the last rule heard stands; with none, nothing runs unasked.
    let rule = probed_rule(offers, &rule_question(request, &args)).await;
    if let Some(stopped) = permission::enforce(offers, &rule, request, &args).await {
        return Some(stopped);
    }
    Some(to_target(offers, mesh, &target, request, &args).await)
}

/// The call that asks the server its rule for `request`. A rule that lets a call run also runs
/// it, and the server may know this machine, so a send names one it cannot have.
fn rule_question(request: &Request, args: &Value) -> Request {
    if request.tool != Tool::SendFiles {
        return request.clone();
    }
    let mut args = args.clone();
    args["machine"] = Value::from("no machine, only asking the rule");
    Request {
        args: args.to_string(),
        ..request.clone()
    }
}

/// The server's rule for `request`, asked by a call without approval. If the server cannot be
/// reached, the last rule heard stands; with none, nothing runs unasked.
async fn probed_rule(offers: &Offers, request: &Request) -> Rule {
    let probe = Request {
        approved: Approved::No,
        ..request.clone()
    };
    match offers.tools.call(&probe).await {
        Ok(Reply::Ask { .. }) => Rule::Ask,
        Ok(Reply::Done { text, failed: true }) if text.starts_with(REFUSED) => Rule::Refuse(text),
        Ok(_) => Rule::Run,
        Err(e) => offers.tools.heard(request.tool).unwrap_or_else(|| {
            Rule::Refuse(format!(
                "Not run: the server could not be asked whether the user allows it ({e:#})."
            ))
        }),
    }
}

/// The most paths one `send_files` call takes.
const MAX_SEND: usize = 20;

/// Offers each path here, then has `target` fetch it: the target's own
/// transfers do the writing, under its own rules about where.
async fn send_files(
    offers: &Offers,
    mesh: &Mesh,
    target: &Machine,
    request: &Request,
    args: &Value,
) -> Reply {
    #[derive(Deserialize)]
    struct Args {
        paths: Vec<String>,
        dest: Option<String>,
    }
    let Ok(Args { paths, dest }) = serde_json::from_value(args.clone()) else {
        return done("Give `machine` and `paths`.", true);
    };
    if paths.is_empty() || paths.len() > MAX_SEND {
        return done(
            format!("Send between 1 and {MAX_SEND} paths at a time."),
            true,
        );
    }
    let Some(transfers) = offers.transfers.get() else {
        return done(
            "This machine's file transfers did not start, so it sends no files.",
            true,
        );
    };
    if target.id == mesh.me() {
        return done("That is this machine.", true);
    }
    let mut lines = Vec::new();
    let mut failed = false;
    for path in &paths {
        let sent = send_one(&transfers, mesh, target, request, path, dest.clone()).await;
        failed |= sent.is_err();
        let (Ok(text) | Err(text)) = sent;
        lines.push(format!("- {path}: {text}"));
    }
    done(lines.join("\n"), failed)
}

/// One path: its text, or why it was not sent.
async fn send_one(
    transfers: &Transfers,
    mesh: &Mesh,
    target: &Machine,
    request: &Request,
    path: &str,
    dest: Option<String>,
) -> Result<String, String> {
    // Held until the target has fetched it.
    let offer = transfers
        .offer_to(path, target.id)
        .await
        .map_err(|e| format!("not sent: {e:#}"))?;
    let TransferReply::Offered {
        content,
        from,
        name,
        executable,
        ..
    } = offer.reply.clone()
    else {
        return Err("not sent: the offer was not made".to_owned());
    };
    let incoming = Incoming {
        from,
        content,
        name,
        executable,
        dest,
    };
    let call = Request {
        args: serde_json::to_string(&incoming).map_err(|e| format!("not sent: {e}"))?,
        ..request.clone()
    };
    match over_the_mesh(mesh, target.id, &call).await {
        Ok(Reply::Done {
            text,
            failed: false,
        }) => Ok(format!("sent to {}: {text}", target.name)),
        Ok(Reply::Done { text, .. }) => Err(format!("not sent: {text}")),
        Ok(other) => Err(format!("not sent: {other:?}")),
        Err(e) => Err(format!("not sent: {e:#}")),
    }
}

/// Scans the project here, then has the machines that sync it (or the one
/// named) scan and wait until they have it.
async fn sync_project(
    offers: &Offers,
    mesh: &Mesh,
    request: &Request,
    args: &Value,
) -> Option<Reply> {
    let project = args.get("project")?.as_str()?;
    let specs: Vec<(String, SyncSpec)> = mesh
        .scan("sync/")
        .into_iter()
        .filter_map(|(key, bytes)| match key {
            Key::Sync(name) => Some((name, SyncSpec::from_bytes(&bytes).ok()?)),
            _ => None,
        })
        .collect();
    let Some((name, spec)) = specs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(project.trim()))
    else {
        if offers.tools.has_server() {
            return None;
        }
        let names: Vec<&str> = specs.iter().map(|(name, _)| name.as_str()).collect();
        return Some(done(
            format!(
                "No synced project called {project}. Synced: {}.",
                names.join(", ")
            ),
            true,
        ));
    };
    let me = mesh.me();
    let mut notes = Vec::new();
    if let Some(transfers) = offers.transfers.get()
        && spec.folders.contains_key(&me)
        && let Err(e) = transfers.syncs().rescan(name).await
    {
        notes.push(format!("not scanned here: {e:#}"));
    }
    let only = args
        .get("machine")
        .and_then(Value::as_str)
        .map(command_name);
    let wanted: Vec<Machine> = machines(mesh)
        .into_iter()
        .filter(|m| m.id != me && spec.folders.contains_key(&m.id))
        .filter(|m| only.as_ref().is_none_or(|only| *only == m.name))
        .collect();
    if wanted.is_empty()
        && only.is_some_and(|only| !machines(mesh).iter().any(|m| m.id == me && m.name == only))
    {
        return Some(done(format!("That machine does not sync {name}."), true));
    }
    let call = Request {
        args: json!({ "project": name }).to_string(),
        ..request.clone()
    };
    let replies = futures::future::join_all(
        wanted
            .iter()
            .map(|machine| to_machine(offers, mesh, machine, &call)),
    )
    .await;
    let mut failed = false;
    let mut text = format!("{name}:");
    for (machine, reply) in wanted.iter().zip(replies) {
        let Reply::Done {
            text: line,
            failed: bad,
        } = reply
        else {
            continue;
        };
        failed |= bad;
        // Writing to a string cannot fail.
        let _ = write!(text, "\n- {}: {line}", machine.name);
    }
    for note in notes {
        let _ = write!(text, "\n{note}");
    }
    Some(done(text, failed))
}

/// `None` (the server's) when there is one, else why there is no such machine.
fn missing(offers: &Offers, mesh: &Mesh, name: &str) -> Option<Reply> {
    if offers.tools.has_server() {
        return None;
    }
    let names: Vec<String> = machines(mesh).into_iter().map(|m| m.name).collect();
    Some(done(
        format!("No machine called {name}. Machines: {}.", names.join(", ")),
        true,
    ))
}

/// The mesh's devices, each with what its own status report says of it
/// when it answers in time.
async fn list(mesh: &Mesh) -> Reply {
    let lines = device_lines(mesh).await;
    done(
        lines
            .into_iter()
            .map(|line| line.text)
            .collect::<Vec<_>>()
            .join("\n"),
        false,
    )
}

/// One device of the mesh as the list shows it.
struct Line {
    name: String,
    text: String,
}

async fn device_lines(mesh: &Mesh) -> Vec<Line> {
    let me = mesh.me();
    futures::future::join_all(mesh.devices().into_iter().map(|status| async move {
        let env = if status.device.id == me {
            Some(env::probe().await)
        } else if status.online {
            peer_env(mesh, status.device.id).await
        } else {
            None
        };
        let here = if status.device.id == me {
            " (this machine)"
        } else {
            ""
        };
        let online = if status.online { "online" } else { "offline" };
        let kind = match status.device.kind {
            DeviceKind::Desktop => "desktop",
            DeviceKind::Node => "node",
        };
        let mut line = format!(
            "- {}{here}: {online}, {kind}",
            command_name(&status.device.name)
        );
        if let Some(env) = env {
            // Writing to a string cannot fail.
            let _ = write!(line, "\n  {}", describe(&env));
        }
        Line {
            name: command_name(&status.device.name),
            text: line,
        }
    }))
    .await
}

/// The server's list with the mesh's devices it does not name added, so a
/// machine that joined by ticket shows. Without a usable server list, the
/// mesh's alone.
async fn merged_list(offers: &Offers, mesh: &Mesh, request: &Request) -> Reply {
    match offers.tools.call(request).await {
        Ok(Reply::Done {
            text,
            failed: false,
        }) => done(merge(&text, device_lines(mesh).await), false),
        _ => list(mesh).await,
    }
}

/// `server`'s list with each of `lines` it does not name added.
fn merge(server: &str, lines: Vec<Line>) -> String {
    let known = listed(server);
    let mut extra: Vec<String> = lines
        .into_iter()
        .filter(|line| !known.contains(&line.name))
        .map(|line| line.text)
        .collect();
    if extra.is_empty() {
        return server.to_owned();
    }
    if known.is_empty() {
        return extra.join("\n");
    }
    extra.insert(0, server.trim_end().to_owned());
    extra.join("\n")
}

/// The machine names a server list shows, one per `- name: ...` line.
fn listed(server: &str) -> BTreeSet<String> {
    server
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .filter_map(|line| line.split_once(':'))
        .map(|(name, _)| command_name(name))
        .collect()
}

/// One line on what the machine is and can do.
fn describe(env: &EnvReport) -> String {
    let mut line = format!(
        "{} {} ({}), {} CPUs, {} GB memory",
        env.os,
        env.arch,
        env.os_version,
        env.cpus,
        env.memory_mb / 1024
    );
    let can: Vec<String> = env
        .capabilities()
        .iter()
        .map(|c| <&str>::from(*c).to_owned())
        .collect();
    if !can.is_empty() {
        let _ = write!(line, "; can: {}", can.join(", "));
    }
    let gpus: Vec<&str> = env.gpus.iter().map(|gpu| gpu.model.as_str()).collect();
    if !gpus.is_empty() {
        let _ = write!(line, "; GPUs: {}", gpus.join(", "));
    }
    let tools: Vec<&str> = env.tools.iter().map(|tool| tool.name.as_str()).collect();
    if !tools.is_empty() {
        let _ = write!(line, "; tools: {}", tools.join(", "));
    }
    line
}

/// What `peer` reports of its machine, or `None` when it does not within
/// `STATUS_WAIT`; the list then says no more of it than its name.
async fn peer_env(mesh: &Mesh, peer: EndpointId) -> Option<EnvReport> {
    let read = async {
        let (_tx, mut rx) = handshake(mesh, peer, status::PROTOCOL).await.ok()?;
        loop {
            if let Report::Env(env) = rx.recv::<Report>().await.ok()?? {
                return Some(env);
            }
        }
    };
    timeout(STATUS_WAIT, read).await.ok().flatten()
}

/// A job `run_on` started on another machine: `task:<n>@<machine>`.
struct Job {
    id: u32,
    machine: String,
}

impl Job {
    fn parse(text: &str) -> Option<Self> {
        let (what, machine) = text.trim().split_once('@')?;
        let id = what.strip_prefix("task:")?.parse().ok()?;
        Some(Self {
            id,
            machine: machine.to_owned(),
        })
    }
}

/// Runs `request` on `target`: here when it is this device, over the mesh
/// otherwise.
async fn to_machine(offers: &Offers, mesh: &Mesh, target: &Machine, request: &Request) -> Reply {
    let reply = if target.id == mesh.me() {
        let caller = Caller::local(offers);
        peer_tools::answer(request.clone(), caller).await
    } else {
        match over_the_mesh(mesh, target.id, request).await {
            Ok(reply) => reply,
            Err(e) => return done(format!("Could not reach {}: {e:#}", target.name), true),
        }
    };
    match (request.tool, reply) {
        (
            Tool::RunOn,
            Reply::Done {
                text,
                failed: false,
            },
        ) => done(name_the_job(&text, &target.name), false),
        (_, reply) => reply,
    }
}

/// `Job id: 7.` becomes `Job id: task:7@box.`, so job_status and cancel
/// find the machine again.
fn name_the_job(text: &str, machine: &str) -> String {
    const MARK: &str = "Job id: ";
    let Some(at) = text.find(MARK) else {
        return text.to_owned();
    };
    let start = at + MARK.len();
    let digits = text[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .count();
    let end = start + digits;
    format!(
        "{}task:{}@{machine}{}",
        &text[..start],
        &text[start..end],
        &text[end..]
    )
}

pub(super) async fn over_the_mesh(
    mesh: &Mesh,
    peer: EndpointId,
    request: &Request,
) -> Result<Reply> {
    let (mut tx, mut rx) = handshake(mesh, peer, PROTOCOL).await?;
    tx.send(request).await?;
    timeout(CALL_LIMIT, rx.recv())
        .await
        .context("the call ran too long")??
        .context("the machine closed the call before answering")
}

/// Opens `tools/1` on `peer`. A link replaced while it opens (two machines
/// dialing each other keep one) is tried again; nothing has run yet.
async fn handshake(mesh: &Mesh, peer: EndpointId, protocol: &str) -> Result<(LinkTx, LinkRx)> {
    let mut tries = HANDSHAKES;
    loop {
        match try_handshake(mesh, peer, protocol).await {
            Err(e) if tries > 1 => {
                log::debug!("opening tools/1 on {peer} failed, trying again: {e:#}");
                tries -= 1;
            }
            Err(e) => return Err(e),
            Ok(Ok(streams)) => return Ok(streams),
            Ok(Err(reason)) => bail!("the machine refused the call: {reason}"),
        }
    }
}

/// The streams, or why the machine turned the call away, which trying
/// again does not change.
async fn try_handshake(
    mesh: &Mesh,
    peer: EndpointId,
    protocol: &str,
) -> Result<Result<(LinkTx, LinkRx), String>> {
    let (send, recv) = open(mesh, peer).await?;
    let (mut tx, mut rx) = link(send, recv);
    tx.send(&Open {
        protocol: protocol.to_owned(),
    })
    .await?;
    let opened: Opened = timeout(OPEN_LIMIT, rx.recv())
        .await
        .context("the machine did not take the call in time")??
        .context("the machine closed the call")?;
    Ok(match opened {
        Opened::Refused { reason } => Err(reason),
        _ => Ok((tx, rx)),
    })
}

/// A stream to `peer`, waiting a while for the link when the mesh holds the
/// device but has no connection to it yet.
async fn open(
    mesh: &Mesh,
    peer: EndpointId,
) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
    let mut updates = mesh.updates();
    let opened = timeout(LINK_WAIT, async {
        loop {
            updates.borrow_and_update();
            match mesh.open_bi(peer).await {
                Ok(streams) => return Ok(streams),
                Err(e) => {
                    if updates.changed().await.is_err() {
                        return Err(e);
                    }
                }
            }
        }
    })
    .await
    .context("the machine is not connected")?;
    Ok(opened?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_id_names_its_machine() {
        let named = name_the_job(
            "Started it in the background. Job id: 7. Check it.",
            "box-b",
        );
        assert_eq!(
            named,
            "Started it in the background. Job id: task:7@box-b. Check it."
        );
        let job = Job::parse("task:7@box-b").expect("a job id");
        assert_eq!((job.id, job.machine.as_str()), (7, "box-b"));
        assert!(Job::parse("7").is_none());
    }

    #[test]
    fn calls_route_by_the_servers_link() {
        // Link up: the server answers what it checks.
        for tool in [Tool::RunOn, Tool::SendFiles, Tool::MachineLog] {
            assert_eq!(route(tool, true, true), Route::Server, "{tool:?}");
            assert_eq!(route(tool, false, false), Route::Direct, "{tool:?}");
            assert_eq!(route(tool, true, false), Route::Direct, "{tool:?}");
        }
        // Not checked by the server: direct whatever the link.
        for tool in [Tool::JobStatus, Tool::Cancel, Tool::SyncProject] {
            assert_eq!(route(tool, true, true), Route::Direct, "{tool:?}");
        }
    }

    #[test]
    fn with_the_link_down_the_last_rule_heard_decides() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let offers = |has_server| {
            Offers::local(
                &[],
                crate::machine::provision::Provision::new(dir.path()),
                has_server,
            )
        };
        let down = offers(true);
        // Never heard: a send asks, a run and a log read go direct.
        assert_eq!(rule_while_down(&down, Tool::SendFiles), Rule::Ask);
        assert_eq!(rule_while_down(&down, Tool::RunOn), Rule::Run);
        assert_eq!(rule_while_down(&down, Tool::MachineLog), Rule::Run);
        let said = |tool, reply: Reply| {
            let request = Request {
                session: "s".to_owned(),
                tool,
                args: "{}".to_owned(),
                approved: Approved::No,
            };
            down.tools.hear(&request, &reply);
        };
        let ask = Reply::Ask {
            app: "Your machines".to_owned(),
            action: "run".to_owned(),
            fields: Vec::new(),
        };
        // Heard: it goes on the rule the server last gave, a refusal included.
        said(Tool::RunOn, ask);
        said(Tool::SendFiles, done("sent", false));
        said(
            Tool::MachineLog,
            done(format!("{REFUSED} Your machines to read only."), true),
        );
        assert_eq!(rule_while_down(&down, Tool::RunOn), Rule::Ask);
        assert_eq!(rule_while_down(&down, Tool::SendFiles), Rule::Run);
        assert!(matches!(
            rule_while_down(&down, Tool::MachineLog),
            Rule::Refuse(_)
        ));
        // A failure that is not a refusal says nothing of the rule.
        said(Tool::SendFiles, done("No machine called x.", true));
        assert_eq!(rule_while_down(&down, Tool::SendFiles), Rule::Run);
        // With no server ever, nothing asks.
        let alone = offers(false);
        for tool in [Tool::RunOn, Tool::SendFiles, Tool::MachineLog] {
            assert_eq!(rule_while_down(&alone, tool), Rule::Run, "{tool:?}");
        }
    }

    fn mesh_line(name: &str) -> Line {
        Line {
            name: name.to_owned(),
            text: format!("- {name}: online, node"),
        }
    }

    const SERVER_LIST: &str = "The user's machines, by the name the other tools take:\n\n\
        - build-box: online; this machine, where you run\n  Linux x86_64\n\n- mac: offline";

    #[test]
    fn the_servers_list_names_its_machines() {
        let names: Vec<String> = listed(SERVER_LIST).into_iter().collect();
        assert_eq!(names, ["build-box", "mac"]);
    }

    #[test]
    fn a_machine_only_the_mesh_knows_joins_the_servers_list() {
        let merged = merge(
            SERVER_LIST,
            vec![mesh_line("build-box"), mesh_line("laptop")],
        );
        assert!(merged.starts_with(SERVER_LIST), "{merged}");
        assert!(merged.ends_with("\n- laptop: online, node"), "{merged}");
        assert_eq!(merged.matches("- build-box:").count(), 1, "{merged}");
        // Nothing to add: the server's text as it is.
        assert_eq!(merge(SERVER_LIST, vec![mesh_line("mac")]), SERVER_LIST);
    }

    #[test]
    fn a_server_with_no_machines_leaves_the_mesh_list() {
        let merged = merge(
            "The user has no machines running beep-node.",
            vec![mesh_line("laptop")],
        );
        assert_eq!(merged, "- laptop: online, node");
    }

    #[test]
    fn a_machine_is_named_in_one_word() {
        assert_eq!(command_name(" Mac Studio "), "mac-studio");
    }

    /// A server that answers every `tools/1` call with `reply`, and the connection to it.
    async fn server_answering(reply: Reply) -> (Offers, impl Sized, tempfile::TempDir) {
        use beep_proto::node::{Open, Opened, link};
        const ALPN: &[u8] = b"beep-test";
        let endpoint = || iroh::Endpoint::builder(iroh::endpoint::presets::Minimal);
        let server = endpoint()
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .expect("the server binds");
        let client = endpoint().bind().await.expect("the node binds");
        let addr = server.addr();
        let serving = tokio::spawn(async move {
            let connection = server
                .accept()
                .await
                .expect("a connection comes")
                .await
                .expect("it connects");
            while let Ok((send, recv)) = connection.accept_bi().await {
                let (mut tx, mut rx) = link(send, recv);
                let _: Open = rx.recv().await.expect("read").expect("an open");
                tx.send(&Opened::Ready {}).await.expect("sent");
                let _: Request = rx.recv().await.expect("read").expect("a call");
                tx.send(&reply).await.expect("sent");
            }
        });
        let connection = client.connect(addr, ALPN).await.expect("the node connects");
        let dir = tempfile::tempdir().expect("a temp folder");
        let offers = Offers::local(
            &[],
            crate::machine::provision::Provision::new(dir.path()),
            true,
        );
        offers.tools.set(Some(connection));
        (
            offers,
            (client, tokio_util::task::AbortOnDropHandle::new(serving)),
            dir,
        )
    }

    fn call_of(tool: Tool) -> Request {
        Request {
            session: "s".to_owned(),
            tool,
            args: "{}".to_owned(),
            approved: Approved::No,
        }
    }

    #[test]
    fn asking_the_rule_for_a_send_cannot_send_through_the_server() {
        let args = json!({ "machine": "box-b", "paths": ["/srv/project/a"] });
        let send = Request {
            args: args.to_string(),
            ..call_of(Tool::SendFiles)
        };
        let asked: Value =
            serde_json::from_str(&rule_question(&send, &args).args).expect("the args parse");
        assert_ne!(asked["machine"], "box-b");
        assert_eq!(asked["paths"], args["paths"]);
    }

    #[tokio::test]
    async fn a_refusing_rule_still_refuses_a_machine_the_server_lacks() {
        let refusal = format!("{REFUSED} Your machines to read only.");
        let (offers, _server, _dir) = server_answering(done(refusal.clone(), true)).await;
        for tool in [Tool::RunOn, Tool::MachineLog, Tool::SendFiles] {
            let rule = probed_rule(&offers, &call_of(tool)).await;
            assert_eq!(rule, Rule::Refuse(refusal.clone()), "{tool:?}");
            let stopped = permission::enforce(&offers, &rule, &call_of(tool), &Value::Null).await;
            assert_eq!(stopped, Some(done(refusal.clone(), true)), "{tool:?}");
        }
        // The server's own failure for a machine it lacks is not a refusal.
        let (offers, _server, _dir) = server_answering(done("No machine called x.", true)).await;
        assert_eq!(probed_rule(&offers, &call_of(Tool::RunOn)).await, Rule::Run);
    }

    #[tokio::test]
    async fn a_probe_the_server_cannot_answer_uses_the_last_rule_heard_else_refuses() {
        let dir = tempfile::tempdir().expect("a temp folder");
        let offers = Offers::local(
            &[],
            crate::machine::provision::Provision::new(dir.path()),
            true,
        );
        // No link at all, no rule heard: nothing runs unasked.
        for tool in [Tool::RunOn, Tool::MachineLog, Tool::SendFiles] {
            assert!(
                matches!(probed_rule(&offers, &call_of(tool)).await, Rule::Refuse(_)),
                "{tool:?}"
            );
        }
        let ask = Reply::Ask {
            app: "Your machines".to_owned(),
            action: "run".to_owned(),
            fields: Vec::new(),
        };
        offers.tools.hear(&call_of(Tool::RunOn), &ask);
        assert_eq!(probed_rule(&offers, &call_of(Tool::RunOn)).await, Rule::Ask);
    }
}
