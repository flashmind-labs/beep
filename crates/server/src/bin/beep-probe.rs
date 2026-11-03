use anyhow::{Context, Result, bail, ensure};
use beep_server::{
    client::{Client, Node, OnNotice},
    config::load_secret_key,
    protocol::{
        CreateNodeToken, GetEvents, GetNodeToken, Health, ListCrons, ListMemories, ListProjectSync,
        ListSessions, SetProjectSync, StreamNotifications, SyncStatus,
    },
};
use iroh::PublicKey;
use std::{fmt::Debug, path::Path, sync::Arc, time::Duration};

const USAGE: &str =
    "usage: beep-probe NODE_ID COMMAND [ARGS]; key file from BEEP_IDENTITY, made if missing

commands:
  health | sessions | memories | crons | notifications
  events RUN_ID
  token NAME                        a setup token for `beep-node setup`
  token-machine TOKEN               the machine id that claimed TOKEN, or nothing
  sync-on PROJECT MACHINE_ID FOLDER turn on PROJECT's sync on a machine
  sync-status PROJECT               one line per machine: id status files pending conflicts error";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(args.len() >= 2, USAGE);
    let identity = std::env::var_os("BEEP_IDENTITY").context("set BEEP_IDENTITY")?;
    let identity = load_secret_key(Some(Path::new(&identity)))?;
    let node_id: PublicKey = args[0].parse().context("invalid node ID")?;
    let on_notice: OnNotice = Arc::new(|notice| {
        show(notice);
        true
    });
    let node = Node::bind(identity, node_id, on_notice).await?;
    let c = node.connect().await?;
    let rest: Vec<&str> = args[2..].iter().map(String::as_str).collect();
    match (args[1].as_str(), rest.as_slice()) {
        ("health", []) => show(c.call(&Health {}).await?),
        ("sessions", []) => show(c.call(&ListSessions {}).await?),
        ("memories", []) => show(c.call(&ListMemories {}).await?),
        ("crons", []) => show(c.call(&ListCrons {}).await?),
        ("notifications", []) => {
            let mut notices = c.stream(&StreamNotifications {}).await?;
            while let Some(notice) = notices.next().await? {
                show(notice);
            }
        }
        ("events", [run_id]) => {
            let request = GetEvents {
                run_id: (*run_id).to_owned(),
                after: 0,
            };
            let mut events = c.stream(&request).await?;
            let wait = Duration::from_secs(45);
            while let Some(event) = tokio::time::timeout(wait, events.next()).await?? {
                println!("{} {:?}", event.seq, event.event);
            }
        }
        ("token", [name]) => {
            let made = c
                .call(&CreateNodeToken {
                    name: Some((*name).to_owned()),
                })
                .await?;
            println!("{}", made.token);
        }
        ("token-machine", [token]) => {
            let claimed = c
                .call(&GetNodeToken {
                    token: (*token).to_owned(),
                })
                .await?;
            if let Some(machine) = claimed {
                println!("{machine}");
            }
        }
        ("sync-on", [project, machine, folder]) => {
            let on = c
                .call(&SetProjectSync {
                    project: (*project).to_owned(),
                    machine_id: (*machine).to_owned(),
                    folder: Some((*folder).to_owned()),
                    on: true,
                })
                .await?;
            show(on);
        }
        ("sync-status", [project]) => sync_status(&c, project).await?,
        _ => bail!(USAGE),
    }
    Ok(())
}

/// Prints how each machine stands with `project`, one line each, for
/// scripts to read.
async fn sync_status(c: &Client, project: &str) -> Result<()> {
    let listed = c.call(&ListProjectSync {}).await?;
    let Some(sync) = listed.iter().find(|sync| sync.project == project) else {
        bail!("{project} is not synced");
    };
    for machine in &sync.machines {
        let status: &'static str = machine.status.into();
        println!(
            "{} {} files={} pending={} conflicts={} error={}",
            machine.machine_id,
            status,
            machine.files,
            machine.pending,
            machine.conflicts.len(),
            machine.error.as_deref().unwrap_or("-").replace(' ', "_"),
        );
    }
    let all_in_sync = sync
        .machines
        .iter()
        .all(|machine| machine.status == SyncStatus::InSync {});
    if !all_in_sync {
        std::process::exit(3);
    }
    Ok(())
}

fn show(reply: impl Debug) {
    println!("{reply:#?}");
}
