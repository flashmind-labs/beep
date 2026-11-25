//! The agent's machine tools answered between the machines of a mesh: the
//! node lists the mesh's devices itself and sends each call to the device it
//! names, over `tools/1`. Between a user's own machines there are no rules to
//! check first.

use super::catalog::REMOTE_RUN;
use crate::agents::calls::command_name;
use crate::peer_stream::{handshake, over_the_mesh};
use crate::agents::reply::{done, fail, ok};
use crate::{
    agents::live::Sent,
    files::transfer::{Incoming, Offer},
    link::{
        Offers,
        peer_tools::{self, Caller},
    },
    machine::env,
};
use anyhow::{Context, Result};
use beep_mesh::{DeviceStatus, Mesh, machine::Device};
use beep_proto::SessionId;
use beep_proto::node::{
    EnvReport,
    status::{self, Report},
    tools::{Reply, Request, Tool},
    transfer::Reply as TransferReply,
};
use beep_proto::watched;
use iroh::EndpointId;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, str::FromStr, time::Duration};
use tokio::{sync::watch, time::timeout};

/// How long a machine has to report itself for `list_machines`.
const STATUS_WAIT: Duration = Duration::from_secs(5);
/// The most paths one `send_files` call takes.
const MAX_SEND: usize = 20;
/// How often a session is shown how far its files have gone.
const SHOW_SENT_EVERY: Duration = Duration::from_secs(1);

/// One device of the mesh.
#[derive(Clone)]
pub(super) struct Machine {
    pub(super) id: EndpointId,
    pub(super) name: String,
}

impl From<DeviceStatus> for Machine {
    fn from(status: DeviceStatus) -> Self {
        Self {
            id: status.device.id,
            name: command_name(&status.device.name),
        }
    }
}

pub(super) fn machines(mesh: &Mesh) -> Vec<Machine> {
    mesh.code_hosts().into_iter().map(Machine::from).collect()
}

/// The machine of `mesh` that `name` names, as agents say it.
pub(super) fn find(mesh: &Mesh, name: &str) -> Option<Machine> {
    let wanted = command_name(name);
    machines(mesh).into_iter().find(|m| m.name == wanted)
}

/// The machine `name` names, or whose endpoint id it is.
pub(super) fn find_or_id(mesh: &Mesh, name: &str) -> Option<Machine> {
    let wanted = command_name(name);
    machines(mesh)
        .into_iter()
        .find(|m| m.id.to_string() == name || m.name == wanted)
}

/// Why there is no machine called `name`.
pub(super) fn no_machine(mesh: &Mesh, name: &str) -> String {
    let all = machines(mesh);
    let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();

    format!("No machine called {name}. Machines: {}.", names.join(", "))
}

pub(super) fn find_named(mesh: &Mesh, name: &str) -> Result<Machine, String> {
    find(mesh, name).ok_or_else(|| no_machine(mesh, name))
}

pub(super) fn find_or_id_named(mesh: &Mesh, name: &str) -> Result<Machine, String> {
    find_or_id(mesh, name).ok_or_else(|| no_machine(mesh, name))
}

/// Whether `mesh` has no machine but this one to run on; phones host nothing.
pub(crate) fn alone(mesh: &Mesh) -> bool {
    let me = mesh.me();

    mesh.inventory().iter().all(|device| match device {
        Device::Phone(_) => true,
        Device::Desktop(status) | Device::Node(status) => status.device.id == me,
    })
}

/// The refusal for a `remote_run` that names, by name or id, the machine the
/// agent runs on: it runs commands on other machines only.
pub(super) fn run_here(mesh: &Mesh, request: &Request) -> Option<Reply> {
    if request.tool != Tool::RunOn {
        return None;
    }

    let args: Value = serde_json::from_str(&request.args).ok()?;
    let named = args.get("machine")?.as_str()?.trim();
    let me = mesh.me();
    let here = machines(mesh).into_iter().find(|m| m.id == me)?;

    if here.name != command_name(named) && named != me.to_string() {
        return None;
    }

    Some(fail(format!(
        "{REMOTE_RUN} runs a command on another machine; {} is this machine. Run the \
         command here with your own shell tool.",
        here.name
    )))
}

/// The reply for `request`, a tool between the machines of `mesh`.
pub(super) async fn call(offers: &Offers, mesh: &Mesh, request: &Request) -> Reply {
    dispatch(offers, mesh, request).await.unwrap_or_else(fail)
}

async fn dispatch(offers: &Offers, mesh: &Mesh, request: &Request) -> Result<Reply, String> {
    let args: Value = serde_json::from_str(&request.args).unwrap_or(Value::Null);

    match request.tool {
        Tool::ListMachines => Ok(ok(device_lines(mesh).await.join("\n"))),
        Tool::RunOn | Tool::SendFiles => {
            let name = args
                .get("machine")
                .and_then(Value::as_str)
                .ok_or("Give `machine`.")?;
            let target = find_named(mesh, name)?;

            if request.tool == Tool::SendFiles {
                send_files(offers, mesh, &target, request, &args).await
            } else {
                Ok(to_machine(offers, mesh, &target, request).await)
            }
        }
        Tool::SyncProject => sync_project(offers, mesh, request, &args).await,
        Tool::JobStatus | Tool::Cancel => {
            let job = args
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<Job>().ok())
                .ok_or("Give the job id remote_run gave, such as task:7@box.")?;
            let target = machines(mesh)
                .into_iter()
                .find(|m| m.name == job.machine)
                .ok_or_else(|| no_machine(mesh, &job.machine))?;
            let sent = Request {
                args: json!({ "id": job.id.to_string() }).to_string(),
                ..request.clone()
            };

            Ok(to_machine(offers, mesh, &target, &sent).await)
        }
        other => Err(format!("{other:?} is not a tool between machines.")),
    }
}

/// Offers each path here, then has `target` fetch it: the target's own
/// transfers do the writing, under its own rules about where.
async fn send_files(
    offers: &Offers,
    mesh: &Mesh,
    target: &Machine,
    request: &Request,
    args: &Value,
) -> Result<Reply, String> {
    #[derive(Deserialize)]
    struct Args {
        paths: Vec<String>,
        dest: Option<String>,
    }

    let Args { paths, dest } = serde_json::from_value(args.clone())
        .map_err(|_| "Give `machine` and `paths`.".to_owned())?;

    if paths.is_empty() || paths.len() > MAX_SEND {
        return Err(format!("Send between 1 and {MAX_SEND} paths at a time."));
    }

    let transfers = offers
        .transfers
        .get()
        .ok_or("This machine's file transfers did not start, so it sends no files.")?;

    if target.id == mesh.me() {
        return Err("That is this machine.".to_owned());
    }

    // Every path is offered before any is fetched, so the call knows all
    // it sends.
    let mut offered = Vec::with_capacity(paths.len());

    for path in &paths {
        let offer = transfers.offer_to(path, target.id).await;
        offered.push(offer.map_err(|e| format!("{e:#}")));
    }

    let total = offered.iter().flatten().map(offered_size).sum();
    let mut before = 0u64;
    let mut lines = Vec::new();
    let mut failed = false;

    for (path, offer) in paths.iter().zip(offered) {
        let sent = match offer {
            Ok(offer) => {
                let sent = Sent {
                    done: before,
                    total,
                };
                let fetched =
                    have_fetched(offers, mesh, target, request, &offer, dest.clone(), sent).await;
                before = before.saturating_add(offered_size(&offer));
                fetched
            }
            Err(why) => Err(why),
        };
        failed |= sent.is_err();
        let line = match sent {
            Ok(text) => format!("- {path}: {text}"),
            Err(why) => format!("- {path}: not sent: {why}"),
        };
        lines.push(line);
    }

    Ok(done(lines.join("\n"), failed))
}

/// The bytes `offer` holds.
fn offered_size(offer: &Offer) -> u64 {
    match &offer.reply {
        TransferReply::Offered { size, .. } => *size,
        _ => 0,
    }
}

/// Has `target` fetch `offer`, showing the caller's session how far it has
/// gone past `sent`: its text, or why it was not sent.
async fn have_fetched(
    offers: &Offers,
    mesh: &Mesh,
    target: &Machine,
    request: &Request,
    offer: &Offer,
    dest: Option<String>,
    sent: Sent,
) -> Result<String, String> {
    let TransferReply::Offered {
        content,
        from,
        name,
        executable,
        ..
    } = offer.reply.clone()
    else {
        return Err("the offer was not made".to_owned());
    };
    let incoming = Incoming {
        from,
        content,
        name,
        executable,
        dest,
    };
    let call = Request {
        args: serde_json::to_string(&incoming).map_err(|e| e.to_string())?,
        ..request.clone()
    };

    let fetched = over_the_mesh(mesh, target.id, &call);
    tokio::pin!(fetched);
    let reply = tokio::select! {
        reply = &mut fetched => reply,
        () = show_sent(offers, &request.session, offer.sent(), sent) => fetched.await,
    };

    match reply {
        Ok(Reply::Done {
            text,
            failed: false,
        }) => Ok(format!("sent to {}: {text}", target.name)),
        Ok(Reply::Done { text, .. }) => Err(text),
        Ok(other) => Err(format!("{other:?}")),
        Err(e) => Err(format!("{e:#}")),
    }
}

/// Shows `session` the bytes sent of one offer after the ones `sent`
/// counts, at most every `SHOW_SENT_EVERY`, until the offer ends or the
/// session cannot be shown.
async fn show_sent(
    offers: &Offers,
    session: &SessionId,
    mut offer: watch::Receiver<u64>,
    sent: Sent,
) {
    loop {
        let done = sent
            .done
            .saturating_add(watched::read_and_update(&mut offer, |bytes| *bytes));
        let shown = Sent { done, ..sent };

        if let Err(e) = offers.agents.sending(session, shown).await {
            log::debug!("send progress not shown: {e:#}");
            return;
        }

        tokio::time::sleep(SHOW_SENT_EVERY).await;

        if offer.changed().await.is_err() {
            return;
        }
    }
}

/// Scans the project here, then has the machines that sync it (or the one
/// named) scan and wait until they have it.
async fn sync_project(
    offers: &Offers,
    mesh: &Mesh,
    request: &Request,
    args: &Value,
) -> Result<Reply, String> {
    let project = args
        .get("project")
        .and_then(Value::as_str)
        .ok_or("Give `project`.")?;
    let specs = super::projects::sync_specs(mesh);
    let (name, spec) = specs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(project.trim()))
        .ok_or_else(|| {
            let names: Vec<&str> = specs.iter().map(|(name, _)| name.as_str()).collect();
            format!(
                "No synced project called {project}. Synced: {}.",
                names.join(", ")
            )
        })?;
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
        return Err(format!("That machine does not sync {name}."));
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

    Ok(done(text, failed))
}

/// Each device of the mesh as the list shows it, with what its own status
/// report says of it when it answers in time.
async fn device_lines(mesh: &Mesh) -> Vec<String> {
    let me = mesh.me();

    futures::future::join_all(mesh.inventory().into_iter().map(|device| async move {
        let label = Into::<&'static str>::into(&device);
        let (status, can_host) = match device {
            Device::Phone(status) => (status, false),
            Device::Desktop(status) | Device::Node(status) => (status, true),
        };
        let env = if !can_host {
            None
        } else if status.device.id == me {
            Some(env::fresh().await)
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
        let mut line = format!(
            "- {}{here}: {online}, {label}",
            command_name(&status.device.name)
        );

        if !can_host {
            line.push_str(
                "\n  Observes and controls machines; does not host sessions or file sync.",
            );
        }

        if let Some(env) = env {
            // Writing to a string cannot fail.
            let _ = write!(line, "\n  {}", describe(&env));
        }

        line
    }))
    .await
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
    let append = |line: &mut String, label: &str, items: &[&str]| {
        if !items.is_empty() {
            let _ = write!(line, "; {label}: {}", items.join(", "));
        }
    };
    let can: Vec<&str> = env
        .capabilities()
        .iter()
        .map(|c| <&str>::from(*c))
        .collect();
    let gpus: Vec<&str> = env.gpus.iter().map(|gpu| gpu.model.as_str()).collect();
    let tools: Vec<&str> = env.tools.iter().map(|tool| tool.name.as_str()).collect();
    append(&mut line, "can", &can);
    append(&mut line, "GPUs", &gpus);
    append(&mut line, "tools", &tools);

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

/// A job `remote_run` started on another machine: `task:<n>@<machine>`.
struct Job {
    id: u32,
    machine: String,
}

impl FromStr for Job {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        let (what, machine) = text.trim().split_once('@').context("missing job machine")?;
        let id = what
            .strip_prefix("task:")
            .context("missing task prefix")?
            .parse()?;

        Ok(Self {
            id,
            machine: machine.to_owned(),
        })
    }
}

/// Runs `request` on `target`: here when it is this device, over the mesh
/// otherwise.
async fn to_machine(offers: &Offers, mesh: &Mesh, target: &Machine, request: &Request) -> Reply {
    let reply = if target.id == mesh.me() {
        peer_tools::answer(request.clone(), Caller::local(offers)).await
    } else {
        match over_the_mesh(mesh, target.id, request).await {
            Ok(reply) => reply,
            Err(e) => return fail(format!("Could not reach {}: {e:#}", target.name)),
        }
    };

    match (request.tool, reply) {
        (
            Tool::RunOn,
            Reply::Done {
                text,
                failed: false,
            },
        ) => ok(name_the_job(&text, &target.name)),
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

#[cfg(test)]
#[path = "../../tests/agents_mcp_machines.rs"]
mod tests;
