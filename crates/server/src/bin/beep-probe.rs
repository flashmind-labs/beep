use anyhow::{Context, Result, bail, ensure};
use beep_server::{
    client::{Node, OnNotice},
    config::load_secret_key,
    protocol::{GetEvents, Health, ListCrons, ListMemories, ListSessions, StreamNotifications},
};
use iroh::PublicKey;
use std::{fmt::Debug, path::Path, sync::Arc, time::Duration};

const USAGE: &str = "usage: beep-probe NODE_ID health|sessions|memories|crons|notifications|events RUN_ID; key file from BEEP_IDENTITY, made if missing";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!((2..=3).contains(&args.len()), USAGE);
    let identity = std::env::var_os("BEEP_IDENTITY").context("set BEEP_IDENTITY")?;
    let identity = load_secret_key(Some(Path::new(&identity)))?;
    let node_id: PublicKey = args[0].parse().context("invalid node ID")?;
    let on_notice: OnNotice = Arc::new(|notice| {
        show(notice);
        true
    });
    let node = Node::bind(identity, node_id, on_notice).await?;
    let c = node.connect().await?;
    match (args[1].as_str(), args.get(2)) {
        ("health", None) => show(c.call(&Health {}).await?),
        ("sessions", None) => show(c.call(&ListSessions {}).await?),
        ("memories", None) => show(c.call(&ListMemories {}).await?),
        ("crons", None) => show(c.call(&ListCrons {}).await?),
        ("notifications", None) => {
            let mut notices = c.stream(&StreamNotifications {}).await?;
            while let Some(notice) = notices.next().await? {
                show(notice);
            }
        }
        ("events", Some(run_id)) => {
            let request = GetEvents {
                run_id: run_id.clone(),
                after: 0,
            };
            let mut events = c.stream(&request).await?;
            let wait = Duration::from_secs(45);
            while let Some(event) = tokio::time::timeout(wait, events.next()).await?? {
                println!("{} {:?}", event.seq, event.event);
            }
        }
        _ => bail!(USAGE),
    }
    Ok(())
}

fn show(reply: impl Debug) {
    println!("{reply:#?}");
}
