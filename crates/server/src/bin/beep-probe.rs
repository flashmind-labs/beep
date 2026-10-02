use anyhow::{Context, Result, ensure};
use beep_server::client::Client;
use bytes::Bytes;
use iroh::{EndpointAddr, PublicKey};
use std::io::{self, Write};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (3..=4).contains(&args.len()),
        "usage: beep-probe NODE_ID METHOD PATH [BODY_FILE]; token from BEEP_TOKEN"
    );
    let token = std::env::var("BEEP_TOKEN").context("set BEEP_TOKEN")?;
    let node_id: PublicKey = args[0].parse().context("invalid node ID")?;
    let addr = EndpointAddr::new(node_id);
    let client = Client::connect(addr).await?;
    let body = args.get(3).map(std::fs::read).transpose()?.map(Bytes::from);
    let mut output_error = None;
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(45),
        client.request(&args[1], &args[2], &token, body, |chunk| {
            let mut stdout = io::stdout().lock();
            if let Err(error) = stdout.write_all(chunk).and_then(|_| stdout.flush()) {
                output_error = Some(error);
            }
        }),
    )
    .await??;
    if let Some(error) = output_error {
        return Err(error.into());
    }
    eprintln!("\niroh {status}");
    ensure!((200..300).contains(&status), "request failed");
    Ok(())
}
