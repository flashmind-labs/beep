use anyhow::{Context, Result, ensure};
use bytes::Bytes;
use neobeep_server::{client::Client, transport::certificates};
use std::{
    io::{self, Write},
    path::Path,
};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        (5..=6).contains(&args.len()),
        "usage: neobeep-probe ADDRESS SERVER_NAME CA_PEM METHOD PATH [BODY_FILE]; token from NEOBEEP_TOKEN"
    );
    let token = std::env::var("NEOBEEP_TOKEN").context("set NEOBEEP_TOKEN")?;
    let mut client = Client::connect(
        args[0].parse()?,
        &args[1],
        certificates(Path::new(&args[2]))?,
    )
    .await?;
    let body = args.get(5).map(std::fs::read).transpose()?.map(Bytes::from);
    let mut output_error = None;
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(45),
        client.request(&args[3], &args[4], &token, body, |chunk| {
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
    eprintln!("\nHTTP/3 {status}");
    ensure!(status.is_success(), "request failed");
    Ok(())
}
