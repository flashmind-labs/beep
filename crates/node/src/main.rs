//! The `beep-node` command line.

use beep_node::{
    agents::Agents, claude::hooks, env, link, local, mcp, service, setup, state, update,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use logforth::{
    append::{Stderr, file::FileBuilder},
    filter::rustlog::RustLogFilterBuilder,
    layout::TextLayout,
};
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::Duration,
};

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
    /// Logs to this file, a new one each day, instead of stderr. The
    /// Windows service uses it, having no console.
    #[arg(long, global = true, hide = true)]
    log_file: Option<PathBuf>,
    /// Stops when the process that started this one exits. The Windows
    /// service uses it, as ending a scheduled task ends only that process.
    #[arg(long, global = true, hide = true)]
    with_parent: bool,
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
    /// Linux, a LaunchAgent on macOS, a scheduled task on Windows.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Installs the newest beep-node over this one and restarts the
    /// service running it, as rerunning the installer does.
    Update {
        /// Only says whether a newer version is out.
        #[arg(long)]
        check: bool,
        /// Restarts at once, without waiting for agent turns to finish.
        #[arg(long)]
        now: bool,
        /// The most minutes to wait for agent turns to finish.
        #[arg(long, default_value_t = 30)]
        wait: u64,
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
    /// Run by a coding agent the node started: the Beep MCP server on
    /// stdio, served by the running node.
    #[command(hide = true)]
    Mcp {
        /// The node's id of the agent's session.
        #[arg(long)]
        session: String,
    },
    /// Lists the Claude Code sessions on this machine started outside Beep.
    Sessions {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Shows the service's log, without the info lines of iroh and the
    /// networking crates under it (warnings and errors still show).
    Logs {
        /// How far back to start, such as 45s, 30m, 1h or 2d.
        #[arg(long, default_value = "1h", value_parser = service::logs::parse_since)]
        since: Duration,
        /// Keeps printing new lines as they are written.
        #[arg(short, long)]
        follow: bool,
        /// Shows only the last N lines, after leaving out the noise.
        #[arg(short = 'n', long, value_name = "N")]
        lines: Option<usize>,
        /// Shows every line, the networking crates' too.
        #[arg(long)]
        all: bool,
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
    /// Writes the unit, LaunchAgent or scheduled task, enables it and
    /// starts it.
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
#[cfg(unix)]
fn stopped() -> Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    })
}

/// Finishes on Ctrl-C, or when the console closes or Windows shuts down.
#[cfg(windows)]
fn stopped() -> Result<impl Future<Output = ()>> {
    use tokio::signal::windows::{ctrl_close, ctrl_shutdown};
    let mut close = ctrl_close()?;
    let mut shutdown = ctrl_shutdown()?;
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = close.recv() => {}
            _ = shutdown.recv() => {}
        }
    })
}

/// The log filter unless `RUST_LOG` says otherwise: the node at info, the
/// networking crates under it (iroh, its QUIC and port mapping) only when
/// something goes wrong, since they log every packet path at info. The log
/// bridge prints their spans as `tracing::span`, which is as noisy.
const LOG_FILTER: &str = "info,iroh=warn,noq=warn,n0=warn,netwatch=warn,netdev=warn,\
                          portmapper=warn,igd_next=warn,tracing::span=warn";

/// Logs to stderr, or to daily files next to `log_file` when given.
fn start_logging(log_file: Option<&Path>) -> Result<()> {
    let filter = || RustLogFilterBuilder::from_default_env_or(LOG_FILTER).build();
    let Some(log_file) = log_file else {
        let stderr = Stderr::default().with_layout(TextLayout::default());
        logforth::starter_log::builder()
            .dispatch(|d| d.filter(filter()).append(stderr))
            .apply();
        return Ok(());
    };
    let folder = log_file.parent().unwrap_or(Path::new("."));
    // `beep-node.log` rolls over to `beep-node.<date>.log`.
    let stem = log_file
        .file_stem()
        .context("--log-file names no file")?
        .to_string_lossy();
    let mut file = FileBuilder::new(folder, stem);
    if let Some(extension) = log_file.extension() {
        file = file.filename_suffix(extension.to_string_lossy());
    }
    let file = file
        .layout(TextLayout::default().no_color())
        .rollover_daily()
        .max_log_files(LOG_FILES)
        .build()
        .with_context(|| format!("opening the log in {}", folder.display()))?;
    logforth::starter_log::builder()
        .dispatch(|d| d.filter(filter()).append(file))
        .apply();
    Ok(())
}

/// The daily log files kept.
const LOG_FILES: NonZeroUsize = NonZeroUsize::new(7).expect("seven is not zero");

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    start_logging(cli.log_file.as_deref())?;
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
            let stop = stopped()?;
            if !cli.with_parent {
                return link::serve(state, cli.workspace.as_deref(), stop).await;
            }
            let parent = service::parent_exited();
            let stop = async move {
                tokio::pin!(stop);
                tokio::select! {
                    () = &mut stop => {}
                    exited = parent => match exited {
                        Ok(()) => log::info!("the service stopped"),
                        Err(e) => {
                            log::warn!("not following the service: {e:#}");
                            stop.await;
                        }
                    },
                }
            };
            link::serve(state, cli.workspace.as_deref(), stop).await
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
        Command::Update { check, now, wait } => {
            let options = update::Options {
                check,
                wait: (!now).then(|| Duration::from_secs(wait.saturating_mul(60))),
                dir,
            };
            Ok(update::update(options).await?)
        }
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
        Command::Mcp { session } => mcp::pipe(&dir, &session).await,
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
        Command::Logs {
            since,
            follow,
            lines,
            all,
        } => {
            let options = service::logs::LogOptions {
                since,
                follow,
                lines,
                all,
            };
            tokio::task::spawn_blocking(move || service::logs::show(&options)).await?
        }
        Command::Env => {
            println!("{:#?}", env::probe().await);
            Ok(())
        }
    }
}
