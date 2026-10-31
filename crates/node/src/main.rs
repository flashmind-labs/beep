//! The `beep-node` command line.

use beep_node::{agents::Agents, claude::hooks, env, link, local, service, state};

use anyhow::Result;
use clap::{Parser, Subcommand};
use logforth::{append::Stderr, filter::rustlog::RustLogFilterBuilder};
use std::path::PathBuf;
use tokio::signal::unix::{SignalKind, signal};

#[derive(Parser)]
#[command(version, about = "Lets Beep run work on this machine")]
struct Cli {
    /// Where the key and settings live. Defaults to the user's config folder.
    #[arg(long, global = true)]
    dir: Option<PathBuf>,
    /// Makes this machine the user's environment: Beep keeps the user's
    /// files in this folder and runs its code there. Used by `run` and
    /// `local`.
    #[arg(long, global = true, env = "BEEP_NODE_WORKSPACE")]
    workspace: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Makes this machine's key and prints a code to link it from the phone.
    Link {
        /// The Beep server's endpoint id.
        #[arg(long)]
        server: String,
        /// What to call this machine, such as "dev server".
        #[arg(long)]
        name: String,
    },
    /// Connects to the server and serves Beep until stopped.
    Run,
    /// Runs as the node bundled with a single-user server: reads the
    /// server's key from a folder both share, leaves this node's key there
    /// for the server to link to its owner, then serves like `run`.
    Local {
        /// The folder shared with the server.
        #[arg(long)]
        link: PathBuf,
        /// What to call this machine.
        #[arg(long, default_value = "Beep server")]
        name: String,
    },
    /// Prints what this machine would report.
    Env,
    /// Runs the node as a systemd user service (Linux).
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Claude Code hooks that tell the node when a session started outside
    /// Beep ends a turn or waits for the user. Install edits Claude Code's
    /// settings and prints what it changed.
    Hooks {
        #[command(subcommand)]
        action: HooksAction,
        /// The settings file to change, such as a project's
        /// `.claude/settings.local.json`. Defaults to the user's.
        #[arg(long, global = true)]
        settings: Option<PathBuf>,
    },
    /// Run by a Claude Code hook: hands the event on stdin to the node.
    #[command(hide = true)]
    Hook {
        /// The hook's event; the input on stdin names it too.
        event: Option<String>,
    },
    /// Lists the Claude Code sessions on this machine started outside Beep.
    Sessions {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
}

#[derive(Subcommand)]
enum HooksAction {
    /// Adds the hooks to Claude Code's settings.
    Install,
    /// Removes them.
    Uninstall,
    /// Says whether they are installed and the node takes them.
    Status,
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Writes the unit, enables it and starts it.
    Install,
    /// Stops the service and removes the unit.
    Uninstall,
}

/// Finishes on Ctrl-C or SIGTERM, which a service manager sends to stop.
fn stopped() -> Result<impl Future<Output = ()>> {
    let mut term = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    logforth::starter_log::builder()
        .dispatch(|d| {
            d.filter(RustLogFilterBuilder::from_default_env_or("info").build())
                .append(Stderr::default())
        })
        .apply();
    let cli = Cli::parse();
    let dir = match cli.dir {
        Some(dir) => dir,
        None => state::default_dir()?,
    };
    match cli.command {
        Command::Link { server, name } => {
            let state = state::State::create(&dir, &server, &name)?;
            link::print_offer(&state)
        }
        Command::Run => {
            let state = state::State::load(&dir)?;
            link::serve(state, cli.workspace.as_deref(), stopped()?).await
        }
        Command::Local { link: shared, name } => {
            let mut stop = Box::pin(stopped()?);
            let state = tokio::select! {
                state = local::prepare(&dir, &shared, &name) => state?,
                () = &mut stop => return Ok(()),
            };
            link::serve(state, cli.workspace.as_deref(), stop).await
        }
        Command::Service {
            action: ServiceAction::Install,
        } => service::install(&dir),
        Command::Service {
            action: ServiceAction::Uninstall,
        } => service::uninstall(),
        Command::Hooks { action, settings } => {
            let settings = match settings {
                Some(path) => path,
                None => hooks::user_settings()?,
            };
            let text = match action {
                HooksAction::Install => {
                    let exe = std::env::current_exe()?;
                    let dir = std::path::absolute(&dir)?;
                    hooks::install(&settings, &hooks::hook_command(&exe, &dir))?
                }
                HooksAction::Uninstall => hooks::uninstall(&settings)?,
                HooksAction::Status => hooks::status(&settings, &dir)?,
            };
            println!("{text}");
            Ok(())
        }
        Command::Hook { event: _ } => {
            hooks::forward(&dir).await;
            Ok(())
        }
        Command::Sessions { limit } => {
            for session in Agents::new(Vec::new()).external_sessions(limit).await? {
                let title = session.title.as_deref().unwrap_or("untitled");
                println!(
                    "{} {} in {}: {}{}",
                    session.id,
                    title,
                    session.cwd,
                    session.presence,
                    session
                        .pid
                        .map(|pid| format!(" (pid {pid})"))
                        .unwrap_or_default()
                );
            }
            Ok(())
        }
        Command::Env => {
            println!("{:#?}", env::probe().await);
            Ok(())
        }
    }
}
