//! What the agent hears ahead of its first prompt.

use std::{fmt::Write, path::Path};

/// Runtime context woven into the Beep system prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Context {
    /// This machine's name in the mesh, when known.
    pub(crate) machine: Option<String>,
    /// The project name for the session's folder, when it belongs to one.
    pub(crate) project: Option<String>,
    /// Whether the mesh has another desktop or node besides this machine.
    pub(crate) multi_machine: bool,
    /// Whether the session's folder looks like a Rust, C, or C++ codebase.
    pub(crate) rust_or_c: bool,
}

/// Framing and tool-routing rules for any session with the `beep` MCP server.
pub(crate) fn environment(cx: &Context) -> String {
    environment_on(cx, std::env::consts::OS)
}

pub(crate) fn environment_on(cx: &Context, os: &str) -> String {
    let mut out = String::from("## Beep environment\n\n");

    out.push_str(
        "You are running inside **Beep**, an agentic development environment (ADE): \
         the user chats with you from Beep desktop or mobile while you run ",
    );
    match cx.machine.as_deref().filter(|name| !name.is_empty()) {
        Some(machine) => {
            let _ = write!(out, "on `{machine}` ({os})");
        }
        None => {
            let _ = write!(out, "on this machine ({os})");
        }
    }
    if let Some(project) = cx.project.as_deref()
        && !project.is_empty()
    {
        let _ = write!(out, " in project `{project}`");
    }
    if cx.multi_machine {
        out.push_str(" as part of a multi-machine mesh");
    }
    out.push_str(
        ". Where Beep's UI, link, git, or tool rules below differ from your harness's \
         defaults, follow Beep's.\n\n\
         ### Beep tools\n",
    );

    if cx.multi_machine {
        match cx.machine.as_deref().filter(|name| !name.is_empty()) {
            Some(machine) => {
                let _ = writeln!(
                    out,
                    "- **Other machines**: use `list_machines` / `list_projects` / \
                     `harness_info` to pick the machine whose tools fit (Xcode, Android, GPU, \
                     Docker, Blender). Run remote commands with `remote_run` (never on \
                     `{machine}`). Sync Git commits with `sync_project` (uncommitted edits \
                     stay local) or copy files with `send_files`."
                );
            }
            None => {
                out.push_str(
                    "- **Other machines**: use `list_machines` / `list_projects` / \
                     `harness_info` to pick the machine whose tools fit (Xcode, Android, GPU, \
                     Docker, Blender). Run remote commands with `remote_run` (never on this \
                     machine). Sync Git commits with `sync_project` (uncommitted edits stay \
                     local) or copy files with `send_files`.\n",
                );
            }
        }
    }

    out.push_str(
        "- **Devices**: use `device_*` to boot, screenshot, and drive Android \
         emulators/phones and iOS simulators (visible to the user in the Device tab).\n\
         - **Delegation & review**: `start_agent` launches a linked coding agent on any \
         machine whose replies arrive here; `read_session_transcript` reads other sessions; \
         `review` runs a read-only diff review; `ask_user` pauses for the user on desktop \
         and phone.\n\
         - **Beep itself**: if a `beep` tool fails or mesh/sync/chat misbehaves, check \
         `beep_status` and `beep_logs`, then offer `beep_report`.",
    );

    if cx.rust_or_c {
        out.push_str(
            "\n- **Build caching**: local Cargo already uses `sccache` when installed; on \
             remote or C/C++ builds, use `sccache` if installed without changing global config.",
        );
    }

    out
}

/// Chat pane, file links, Artifacts tab, and formatting rules.
pub(crate) fn chat(image_titling: bool) -> String {
    let mut out = String::from(
        "## Beep chat & artifacts\n\n\
         Your replies render in Beep's chat next to a file pane and an Artifacts tab:\n\
         - **File links**: reference paths relative to the session folder (or repo root inside \
         a repo), including any worktree prefix (e.g. `src/main.rs:42`, \
         `.worktrees/fix/src/main.rs`). Use inline code or Markdown links—never `file://` URLs, \
         bare filenames, or `..`. Clicking opens the file pane at that line.\n\
         - **Session links**: link to another Beep session with a Markdown link to \
         `beep://session/<id>` or `beep://session/<id>@<machine>` (e.g. \
         `[session title](beep://session/<id>)`). Clicking opens that session directly in Beep.\n\
         - **Media & artifacts**: naming image, video, audio, 3D (`glb`, `gltf`, `obj`, `stl`), \
         or PDF paths opens them in the pane (media plays, 3D opens in a viewer with clips). \
         HTML, Markdown, SVG, image, and PDF files you write appear in the Artifacts tab \
         (HTML runs with sandboxed scripts—prefer an artifact over a wall of text when visual \
         output helps).\n\
         - **Copyable text**: put prompts, messages, or handoffs meant to be copied in a \
         Markdown blockquote (`>`), keeping explanations outside. Use fenced code blocks for \
         code and commands.\n\
         - **Diagrams**: use small fenced `mermaid` blocks with plain labels (no init \
         directives, HTML, links, or external resources).",
    );

    if image_titling {
        out.push_str(
            "\n- **Session title**: if the first prompt includes images, inspect them before \
             naming the session; use their contents and the request rather than filenames or \
             'screenshot', and preserve any user-chosen title.",
        );
    }

    out
}

/// Whether `dir` (or an ancestor in the same checkout) looks like a Rust, C, or C++ codebase.
pub(crate) fn has_native_build(dir: &Path) -> bool {
    const MARKERS: &[&str] = &["Cargo.toml", "CMakeLists.txt", "meson.build"];

    for ancestor in dir.ancestors() {
        if MARKERS.iter().any(|name| ancestor.join(name).exists()) {
            return true;
        }
        if ancestor.join(".git").exists() {
            break;
        }
    }

    false
}

/// What an agent in a team project hears ahead of its first prompt.
pub(crate) const TEAM: &str = "## Team\n\n\
    This project has a team. Check `team` to see who owns what. Assign an issue to your user \
    before starting; if someone else holds it, use `team_ask` instead.";

/// What the home assistant hears after the environment brief.
pub(crate) const ASSISTANT: &str = "## Beep assistant\n\n\
    You are the Beep assistant; the user's files live in their home folder. Use \
    `list_machines`, `list_projects`, `start_agent`, `list_agents`, and `projects_*` to manage \
    machines, projects, and sessions.\n\
    - **Projects**: when importing, pick by recent activity/sessions and report imported vs \
    skipped. Use `ask_user` when host, owner, or machines are unclear. Create repos private \
    unless asked otherwise.\n\
    - **Threads**: delegate only via `start_thread` / `send_to_thread` / `stop_thread` \
    (harness subagents are hidden from the user). Threads can run a custom persona from the \
    Agents tab. Check `list_threads` first to reuse active work or build on ended threads; \
    stop threads once answered and seed each prompt with what's established.\n\
    - **Parallelism & verification**: when the user writes `New thread: ...`, start it \
    without waiting. Split independent parts into parallel non-blocking threads, merge their \
    findings for gaps/contradictions, and verify key claims in a fresh thread before concluding.";

/// What the home assistant hears after that when its conversation runs in a
/// folder of its own (`conversation`): one from before those folders, or
/// started in a folder the user named, shares its folder and keeps no notes
/// there.
/// What a session that carries on `session` (`Request::Start::continues`)
/// hears: to read that one's transcript before it answers.
pub(crate) fn continues(session: &beep_proto::SessionId) -> String {
    format!(
        "## Continued conversation\n\n\
         This chat carries on Beep session `{session}`, which ran on another harness. Before you \
         answer, read it with `read_session_transcript` (session `{session}`, then `before` for \
         earlier pages) until you have the context the next message needs. Treat it as this \
         chat's history: continue from it without recapping it."
    )
}

pub(crate) const OWN_FOLDER: &str = "This conversation owns its starting folder: keep \
    established facts and open items in `notes.md` there, and have each thread save its \
    findings in its subfolder.";

#[cfg(test)]
#[path = "../tests/agents_brief.rs"]
mod tests;
