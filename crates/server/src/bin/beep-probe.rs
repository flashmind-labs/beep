use anyhow::{Context, Result, bail, ensure};
use beep_server::{
    client::Client,
    protocol::{Reply, Request},
};
use iroh::{EndpointAddr, PublicKey};
use std::time::Duration;

const USAGE: &str = "usage: beep-probe NODE_ID health|sessions|memories|crons|integrations|notifications|events RUN_ID; token from BEEP_TOKEN";

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!((2..=3).contains(&args.len()), USAGE);
    let token = std::env::var("BEEP_TOKEN").context("set BEEP_TOKEN")?;
    let node_id: PublicKey = args[0].parse().context("invalid node ID")?;
    let request = match (args[1].as_str(), args.get(2)) {
        ("health", None) => Request::Health {},
        ("sessions", None) => Request::ListSessions {},
        ("memories", None) => Request::ListMemories {},
        ("crons", None) => Request::ListCrons {},
        ("integrations", None) => Request::ListIntegrations {},
        ("notifications", None) => Request::StreamNotifications {},
        ("events", Some(run_id)) => Request::GetEvents {
            run_id: run_id.clone(),
            after: 0,
        },
        _ => bail!(USAGE),
    };
    let client = Client::connect(EndpointAddr::new(node_id), &token).await?;
    let mut replies = client.stream(&request).await?;
    while let Some(frame) = tokio::time::timeout(Duration::from_secs(45), replies.next()).await?? {
        match frame {
            Reply::Event { seq, event } => println!("{seq} {event:?}"),
            Reply::Heartbeat {} => eprintln!("heartbeat"),
            other => println!("{other:?}"),
        }
    }
    Ok(())
}
