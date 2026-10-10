//! Sends one admin command to the server running on this machine, over its
//! admin socket, and prints the reply. In the container:
//! `docker compose exec server beep-admin rescan [USER_ID]`.

use anyhow::{Context, Result, ensure};
use std::{
    env,
    io::{Read, Write},
    os::unix::net::UnixStream,
};

const USAGE: &str =
    "usage: beep-admin rescan [USER_ID]; socket from BEEP_ADMIN_SOCKET, /data/admin.sock if unset";
const DEFAULT_SOCKET: &str = "/data/admin.sock";

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    ensure!(!args.is_empty(), USAGE);
    let socket = env::var("BEEP_ADMIN_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.to_owned());
    let mut stream =
        UnixStream::connect(&socket).with_context(|| format!("connecting to {socket}"))?;
    writeln!(stream, "{}", args.join(" "))?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    print!("{reply}");
    Ok(())
}
