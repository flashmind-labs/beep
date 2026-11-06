//! The `beep-node` command line.

use beep_node::{
    agents::{Agents, claude::hooks, mcp},
    link::{self, local, peers, state},
    machine::{env, service, setup, update},
};

use anyhow::{Context, Result};
use beep_proto::logging::{self, RollingFile};
use beep_report::Period;
use clap::{FromArgMatches, Parser, Subcommand};
use std::{
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{runtime::Builder, sync::oneshot};
use tokio_util::task::AbortOnDropHandle;

mod cli;

#[derive(Parser)]
#[command(version, about = "Lets Beep run work on this machine")]
struct Cli {
    /// Where the key and settings live; the user's config folder by default.
    #[arg(long, global = true)]
    dir: Option<PathBuf>,
    /// Makes this folder the user's workspace, for `run` and `local`: Beep
    /// keeps the user's files in it and runs their code there.
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
    /// Links this machine to a server account with a setup token.
    ///
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
        /// The folder to look for git checkouts in, such as ~/Projects.
        /// Defaults to home.
        #[arg(long)]
        search: Option<String>,
        /// Stops after linking instead of serving.
        #[arg(long, conflicts_with = "foreground")]
        no_run: bool,
        /// Serves in the foreground until stopped, like `run`, instead of
        /// installing the service.
        #[arg(long)]
        foreground: bool,
    },
    /// Links to an older server with a code (legacy).
    ///
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
    /// Serves Beep in this terminal until stopped.
    ///
    /// Serves Beep until stopped: to the server when this machine is linked to
    /// one, and to the devices of its mesh. Makes this machine's key on the
    /// first run.
    Run,
    /// This machine's mesh of devices.
    ///
    /// A desktop starts a mesh and brings machines in; a node can only join.
    Mesh {
        #[command(subcommand)]
        action: MeshAction,
    },
    /// Runs as the node bundled with a single-user server (legacy).
    ///
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
    /// Prints what this machine reports to Beep.
    Env,
    /// Installs or removes the background service.
    ///
    /// Runs the node in the background as a service: a systemd user unit on
    /// Linux, a LaunchAgent on macOS, a scheduled task on Windows.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Installs the newest beep-node and restarts the service.
    ///
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
        #[arg(long, default_value_t = update::WAIT_MINUTES)]
        wait: u64,
    },
    /// Installs or removes the Claude Code hooks.
    ///
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
    /// Started by the node: keeps its terminals running while it restarts.
    #[cfg(unix)]
    #[command(hide = true)]
    PtyHost,
    /// Lists the Claude Code sessions started outside Beep.
    Sessions {
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Shows the service's log.
    ///
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
    /// Shows the node, its peers and project sync.
    ///
    /// Asks the node running from `--dir`: its version and how long it has
    /// run, this machine's name and id, the mesh and its devices (online, or
    /// when last seen), and for each project whether it is in sync, syncing,
    /// waiting for Syncthing, missing its folder, or not syncing and why.
    /// Says so when no node is running.
    #[command(visible_aliases = ["s", "st"])]
    Status,
    /// Sends a problem report with every machine's log to Beep.
    ///
    /// Collects the log, status, version and OS of every machine of the
    /// mesh through the node running from `--dir` (this machine alone when
    /// none runs), shows what it will send, and sends it once you agree.
    /// Afterwards it can open a GitHub issue with your own `gh`, without
    /// logs or machine names.
    Report {
        /// How far back the logs reach: hour, today or day.
        #[arg(long, default_value = "today")]
        period: Period,
        /// Sends without asking, and opens no GitHub issue.
        #[arg(long)]
        yes: bool,
        /// What happened; asked for when not given.
        #[arg(long, value_name = "TEXT")]
        message: Option<String>,
    },
    /// Prints the version.
    Version,
}

#[derive(Subcommand)]
enum MeshAction {
    /// Joins the mesh a desktop's ticket names.
    ///
    /// Joins in the node running from this folder, and starts it as a
    /// service first when none runs.
    Join {
        /// The ticket, or `-` to read it from stdin, which keeps it out of
        /// process listings and shell history.
        ticket: String,
        /// The folder to look for git checkouts in, as for `setup`.
        #[arg(long)]
        search: Option<String>,
    },
    /// Lists the mesh's devices and which are online.
    #[command(visible_alias = "peers")]
    Devices,
    /// Leaves the mesh and deletes what this machine kept of it.
    ///
    /// Afterwards it can join another. The node keeps running.
    Leave,
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
    /// Writes the unit, init script, LaunchAgent or scheduled task, enables it and
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

/// How long the node has to stop once told to, before it exits anyway. A
/// service manager kills it after its own limit (the systemd unit's is 20 s).
const STOP_LIMIT: Duration = Duration::from_secs(10);
/// How long the exit waits for blocking work still running, such as the
/// `systemctl restart` an update started, which itself waits on this exit.
const EXIT_LIMIT: Duration = Duration::from_secs(1);

/// Finishes once the node is told to stop. The signal is watched on a task
/// of its own, so it is logged the moment it arrives even while the node is
/// busy, and it starts the `STOP_LIMIT` deadline.
fn stopped() -> Result<impl Future<Output = ()>> {
    let signal = signal()?;
    let (told, stop) = oneshot::channel();
    let watch = AbortOnDropHandle::new(tokio::spawn(async move {
        let name = tokio::select! {
            name = signal => name,
            () = update::reexec::asked() => "restarting on the new binary",
        };
        log::info!("stopping: {name}");
        exit_after(STOP_LIMIT);
        // Nobody listens once the node stopped another way.
        let _ = told.send(());
    }));
    Ok(async move {
        let _watch = watch;
        // An error means the watch is gone, which only the runtime stopping
        // does: stop too.
        let _ = stop.await;
    })
}

/// Exits the process `limit` from now, unless it ended before. Only the
/// first call counts. A plain thread, so neither a stop step that hangs nor
/// async workers that do can hold it.
fn exit_after(limit: Duration) {
    static ARMED: AtomicBool = AtomicBool::new(false);
    if ARMED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = thread::Builder::new()
        .name("stop-deadline".to_owned())
        .spawn(move || {
            thread::sleep(limit);
            log::warn!("stopping took over {} s: exiting now", limit.as_secs());
            reexec();
            process::exit(1);
        });
    if let Err(e) = spawned {
        log::warn!("stopping has no deadline: {e}");
    }
}

/// Finishes on Ctrl-C or SIGTERM, which a service manager sends to stop,
/// with the signal's name.
#[cfg(unix)]
fn signal() -> Result<impl Future<Output = &'static str>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "Ctrl-C",
            _ = term.recv() => "SIGTERM",
        }
    })
}

/// Finishes on Ctrl-C, or when the console closes or Windows shuts down,
/// saying which.
#[cfg(windows)]
fn signal() -> Result<impl Future<Output = &'static str>> {
    use tokio::signal::windows::{ctrl_close, ctrl_shutdown};
    let mut close = ctrl_close()?;
    let mut shutdown = ctrl_shutdown()?;
    Ok(async move {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "Ctrl-C",
            _ = close.recv() => "the console closed",
            _ = shutdown.recv() => "Windows shuts down",
        }
    })
}

/// The log filter unless `RUST_LOG` says otherwise: the node at info, the
/// networking crates under it (iroh, its QUIC and port mapping) only when
/// something goes wrong, since they log every packet path at info.
const LOG_FILTER: &str =
    "info,iroh=warn,noq=warn,n0=warn,netwatch=warn,netdev=warn,portmapper=warn,igd_next=warn";

/// Logs to stderr, or to `log_file` when given, a new one each day.
fn start_logging(log_file: Option<&Path>) -> Result<()> {
    let file = log_file
        .map(|path| RollingFile::new(path, LOG_FILES))
        .transpose()?;
    logging::start(LOG_FILTER, file.is_none(), file)
}

/// The daily log files kept.
const LOG_FILES: usize = 7;

fn main() -> Result<()> {
    let runtime = Builder::new_multi_thread().enable_all().build()?;
    let done = runtime.block_on(run());
    // Dropped as is, the runtime would wait for every blocking task still
    // running, however long.
    runtime.shutdown_timeout(EXIT_LIMIT);
    reexec();
    done
}

/// Runs the new binary in place of this process when an update asked for
/// it, having no service to restart the node. Returns only when it could
/// not, which is logged.
fn reexec() {
    #[cfg(unix)]
    if let Some(exe) = update::reexec::next() {
        let e = update::reexec::exec(exe);
        log::error!("could not start {}: {e}", exe.display());
    }
}

async fn run() -> Result<()> {
    let cli =
        Cli::from_arg_matches(&cli::help::command().get_matches()).unwrap_or_else(|e| e.exit());
    start_logging(cli.log_file.as_deref())?;
    #[cfg(unix)]
    beep_node::machine::raise_open_files();
    let dir = match cli.dir {
        Some(dir) => dir,
        None => state::default_dir()?,
    };
    match cli.command {
        Command::Setup {
            token,
            name,
            search,
            no_run,
            foreground,
        } => {
            let search = search.as_deref().map(state::Search::parse);
            let (state, claimed) = setup::setup(&dir, &token, name.as_deref(), search).await?;
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
        Command::Mesh { action } => match action {
            MeshAction::Join { ticket, search } => {
                // Refused before the ticket is spent, as `setup` does.
                let search = search.as_deref().map(state::Search::parse);
                if let Some(search) = &search {
                    search.existing_root().context("--search")?;
                }
                let ticket = if ticket == "-" {
                    let mut text = String::new();
                    tokio::io::AsyncReadExt::read_to_string(&mut tokio::io::stdin(), &mut text)
                        .await?;
                    text
                } else {
                    ticket
                };
                // A fresh machine has no node yet: start it in the background,
                // as `setup` does, before the ticket is sent.
                if !peers::is_running(&dir) {
                    // A service for another folder is not replaced.
                    let action = service::join_action(&dir)?;
                    // The service needs the folder and the key to exist.
                    state::State::load_or_start(&dir, &setup::host_name().await)?;
                    let failed = "no node is running and the service could not be started; run \
                         `beep-node run` in another terminal, then join again";
                    match action {
                        service::Action::Install => {
                            let described =
                                in_background(&dir, cli.workspace.as_deref()).context(failed)?;
                            println!("{described}");
                        }
                        // Its unit stays as it is, with its own workspace.
                        service::Action::Start => {
                            let manager = service::start().context(failed)?;
                            println!("Started the stopped {manager} service.");
                        }
                    }
                }
                peers::join_when_up(&dir, &ticket, peers::START_LIMIT).await?;
                if let Some(search) = &search {
                    state::save_search(&dir, search)?;
                }
                println!("Joined. The node picked it up.");
                Ok(())
            }
            MeshAction::Devices => {
                for line in peers::devices(&dir).await? {
                    println!("{line}");
                }
                Ok(())
            }
            MeshAction::Leave => peers::leave(&dir).await,
        },
        Command::Run => {
            let state = state::State::load_or_start(&dir, &setup::host_name().await)?;
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
                        Ok(()) => {
                            log::info!("stopping: the service stopped");
                            exit_after(STOP_LIMIT);
                        }
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
        #[cfg(unix)]
        Command::PtyHost => beep_node::run_terminal_host(&dir).await,
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
        Command::Status => {
            let text = match peers::local_status(&dir).await? {
                Some(status) => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|since| since.as_secs())
                        .unwrap_or(0);
                    cli::status::render(&status, now)
                }
                None => cli::status::not_running(),
            };
            print!("{text}");
            Ok(())
        }
        Command::Report {
            period,
            yes,
            message,
        } => cli::report::run(&dir, period, yes, message).await,
        Command::Version => {
            println!("beep-node {}", cli::status::VERSION);
            Ok(())
        }
    }
}
