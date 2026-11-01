//! The `beep-node` command line.

use beep_node::{agents::Agents, claude::hooks, env, link, local, service, setup, state};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use logforth::{append::Stderr, filter::rustlog::RustLogFilterBuilder};
use std::path::{Path, PathBuf};
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
    /// Links this machine to the account that made `token` in Beep ("Add a
    /// machine"), then runs the node in the background as a service that
    /// starts with the machine (see `service`). Where there is no service
    /// manager to use, as in a container, it serves in the foreground.
    Setup {
        /// The setup token, `bk1-...`.
        token: String,
        /// What to call this machine, such as "dev server". Defaults to the
        /// name given in Beep, else the host name.
        #[arg(long)]
        name: Option<String>,
        /// Stops after linking instead of serving.
        #[arg(long, conflicts_with = "foreground")]
        no_run: bool,
        /// Serves in the foreground until stopped, like `run`, instead of
        /// installing the service.
        #[arg(long)]
        foreground: bool,
    },
    /// The old way, kept for older servers: makes this machine's key and
    /// prints a code to paste into the phone. Prefer `setup`.
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
    /// Runs the node in the background as a service: a systemd user unit on
    /// Linux, a LaunchAgent on macOS.
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
    /// Writes the unit or LaunchAgent, enables it and starts it.
    Install,
    /// Stops the service and removes it.
    Uninstall,
}

/// Installs and starts the node's service, saying where it runs; the error
/// says why it could not.
fn in_background(dir: &Path, workspace: Option<&Path>) -> Result<String> {
    let manager = service::manager()?;
    let launch = service::Launch::new(dir, workspace)?;
    let installed = manager
        .install(&launch)
        .context("could not start the service")?;
    installed.describe()
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
        Command::Setup {
            token,
            name,
            no_run,
            foreground,
        } => {
            let (state, claimed) = setup::setup(&dir, &token, name.as_deref()).await?;
            if no_run {
                println!(
                    "Linked to {}. Start it with `beep-node service install`, or \
                     `beep-node run` to serve here.",
                    claimed.account
                );
                return Ok(());
            }
            println!("Linked to {}.", claimed.account);
            if !foreground {
                match in_background(&dir, cli.workspace.as_deref()) {
                    Ok(described) => {
                        println!("{described}");
                        return Ok(());
                    }
                    Err(why) => println!("Serving here in the foreground: {why:#}."),
                }
            }
            link::serve(state, cli.workspace.as_deref(), stopped()?).await
        }
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
        } => service::install(&dir, cli.workspace.as_deref()),
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
