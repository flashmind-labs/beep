import { S, windowHooks } from "../bar/core.ts";
import { forgetView } from "../agentpane/view-memory.ts";
import { onPhone, WINDOWS } from "../ui/platform.ts";
import { NEW_WINDOW } from "../project/projectwindow.ts";
// The right-click menu of a coding agent session, shared by every row that shows one.
import { confirmSheet, openMenu, type MenuItem, type MenuLine } from "../ui/menu.tsx";
import { writeClipboard } from "../ui/parts.tsx";
import { invoke } from "../ui/tauri.ts";
import { openInEditor } from "../project/editors.ts";
import { chatFolder } from "./nesting-state.ts";
import type { AgentRow } from "./agents.ts";
import { parentSession, sessions } from "./sessions.ts";
import { type SideWork, sessionOffers, stopAllAsks } from "./sessionmenu-state.ts";
import { AG, AGENT_ACTIVE, openOf, where } from "./agents.ts";
import { sameSession } from "./running-state.ts";
import { workingSubagents } from "./logview.tsx";
import { draftNewThread, resumeEndedSession, stopAllWork } from "./messages.ts";
import { runs, transcriptKey, transcripts } from "./transcript-cache-state.ts";
import { closeSession } from "./closing.ts";
import { forgetTranscript } from "./session-follow.ts";
import { forgetLog } from "./session.tsx";

/// Stops the turn of `s`, its subagents, background commands and the agents it started; asks
/// first while any of those run.
function stopAll(s: AgentRow) {
  const o = openOf(s);
  const open = sameSession(AG.open, s) && !!AG.page;
  const run = () => stopAllWork(s);
  const work: SideWork = {
    // The node's count reaches sessions that are not open; the open log may show one sooner.
    subagents: Math.max((open ? AG.page : transcripts.peek(transcriptKey(o))?.page)?.session.subagents ?? s.subagents ?? 0, open ? workingSubagents() : 0),
    tasks: transcripts.tasks(transcriptKey(o)).filter(runs).length,
    started: sessions.filter(row => { const parent = parentSession(row); return AGENT_ACTIVE.has(row.status) && !!parent && parent.machine_id === s.machine_id && parent.id === s.id; }).length,
  };
  if (!stopAllAsks(work)) return void run().catch(err => console.warn("stop all:", err));
  confirmSheet({
    title: "Stop all of this session's work?",
    text: "Stops the turn, its subagents, background commands and the agents it started.",
    note: "While subagents run, the agent process is stopped. Your next message resumes it.",
    action: "Stop all",
    run,
  });
}

/// Opens the menu for `s` where `e` was clicked. `open` is what clicking the row does.
export async function sessionMenu(e: MouseEvent, s: AgentRow, open: (s: AgentRow) => void) {
  e.preventDefault();
  e.stopPropagation();
  const at = where(openOf(s));
  const build = (folder: string | null, reachable: boolean) => {
    const offers = sessionOffers(s, reachable);
    const openGroup: MenuLine[] = [{ label: "Open", icon: "move", on: () => open(s) }];
    if (!onPhone() && S.view === "agent" && AG.open && !sameSession(AG.open, s)) {
      openGroup.push({ label: "Open beside", icon: "splitPane", on: () => void import("../agentpane/media.ts").then(m => m.openSessionBeside({
        machine_id: s.machine_id, machine_name: s.machine_name, agent: s.agent, id: s.id,
        title: s.title ?? null, cwd: s.cwd, status: s.status, external: s.external,
      })) });
    }
    const parent = parentSession(s);
    if (parent) openGroup.push({ label: "Open parent", icon: "back", on: () => open(parent) });
    if (!s.external && s.assistant && !s.reviewer) {
      if (!onPhone()) openGroup.push({ label: NEW_WINDOW, icon: "openWindow", on: () => void import("../center/sidebar.tsx").then(({ openChatWindow }) => openChatWindow(s)) });
      openGroup.push({ label: "New thread", icon: "chat", on: () => { draftNewThread(s); open(s); } });
      const chatDir = chatFolder(s.cwd);
      if (chatDir) openGroup.push({ label: "Skills", icon: "skill", chevron: true, on: () => void import("../project/project-skills.tsx").then(({ openChatSkills }) => openChatSkills(chatDir, s.title || "this chat")) });
    }

    const runGroup: MenuLine[] = [];
    if (offers.stop) {
      runGroup.push({ label: "Stop turn", icon: "stop", detail: onPhone() ? undefined : (WINDOWS ? "Ctrl+." : "⌘."), on: () => void invoke("agent_steer", { ...at, steer: "cancel" }).catch(err => console.warn("stop turn:", err)) });
      runGroup.push({ label: "Stop all", icon: "stopAll", on: () => stopAll(s) });
    }
    if (offers.resume) {
      runGroup.push({ label: "Resume", icon: "run", on: () => void resumeEndedSession(openOf(s)).then(started => open(started.session), err => console.warn("resume:", err)) });
    }

    const workspaceGroup: MenuLine[] = [];
    if (folder && !onPhone()) {
      workspaceGroup.push({ label: "Open in editor", icon: "paneText", on: () => void openInEditor(folder) });
      workspaceGroup.push({ label: "Open folder", icon: "folder", on: () => void invoke("open_folder", { path: folder }).catch(err => console.warn("open folder:", err)) });
    }
    if (offers.delete) {
      const targets = windowHooks?.sessionDestinations?.(s) ?? [];
      if (targets.length) workspaceGroup.push({ label: "Move to project", icon: "move", chevron: true, on: () => {
        openMenu(e, "Move to project", [
          { label: "Back", icon: "back", on: () => void sessionMenu(e, s, open) },
          { kind: "separator" },
          ...targets.map(target => ({ label: target.name, icon: "folder" as const, on: () => confirmSheet({
            title: `Move to ${target.name}?`,
            text: "Keeps the conversation and changes its working folder. Files stay where they are. Resume the session after moving it.",
            action: "Move",
            run: async () => {
              await invoke("agent_relocate", { ...at, folder: target.folder });
            },
          }) })),
        ]);
      } });
    }
    workspaceGroup.push({ label: "Copy id", icon: "copy", on: () => void writeClipboard(s.id) });

    const dangerGroup: MenuLine[] = [];
    if (offers.close) {
      dangerGroup.push({ label: "Close session", icon: "close", warn: true, on: () => void closeSession(s) });
    }
    if (offers.delete) {
      dangerGroup.push({
        label: "Delete",
        icon: "remove",
        warn: true,
        on: () => confirmSheet({
          title: "Delete this session?",
          text: `Its conversation is removed from ${s.machine_name || "the machine"} for good. A terminal or IDE that has it open is closed.`,
          action: "Delete",
          run: async () => {
            await invoke("agent_delete", at);
            forgetView(s.machine_id, s.id);
            forgetTranscript(openOf(s));
            forgetLog(s);
          },
        }),
      });
    }

    const items: MenuItem[] = [{ kind: "section", label: "Open", items: openGroup }];
    if (runGroup.length) items.push({ kind: "separator" }, { kind: "section", label: "Run", items: runGroup });
    if (workspaceGroup.length) items.push({ kind: "separator" }, { kind: "section", label: "Workspace", items: workspaceGroup });
    if (dangerGroup.length) items.push({ kind: "separator" }, ...dangerGroup);
    return items;
  };
  // Only a folder this Mac has can be opened here.
  const folder = invoke<string | null>("session_folder_here", { machineId: s.machine_id, cwd: s.cwd }).catch(() => null);
  const reachable = invoke<boolean>("agent_deletable", { machineId: s.machine_id }).catch(() => false);
  // Shown at once with what is known; the folder and the mesh fill in a moment later.
  const update = openMenu(e, s.title || "Session", build(null, false), s.machine_name || undefined);
  update(build(await folder, await reachable));
}
