import { S, windowHooks } from "../bar/core.ts";
import { forgetView } from "../agentpane/view-memory.ts";
import { onPhone } from "../ui/platform.ts";
import { NEW_WINDOW } from "../project/projectwindow.ts";
// The right-click menu of a coding agent session, shared by every row that shows one.
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.tsx";
import { writeClipboard } from "../ui/parts.tsx";
import { invoke } from "../ui/tauri.ts";
import { openInEditor } from "../project/editors.ts";
import { chatFolder } from "./nesting-state.ts";
import type { AgentRow } from "./agents.ts";
import { parentSession, loadSessions, sessions } from "./sessions.ts";
import { type SideWork, sessionOffers, stopAllAsks } from "./sessionmenu-state.ts";
import { AG, AGENT_ACTIVE, openOf, where } from "./agents.ts";
import { sameSession } from "./running-state.ts";
import { workingSubagents } from "./logview.tsx";
import { runs } from "./tasks.tsx";
import { draftNewThread, drainOutbox, resumeEndedSession, stopAllWork } from "./messages.ts";
import { conversationKey } from "./conversation-state.ts";
import { transcriptKey } from "./transcript-cache-state.ts";
import { closeSession } from "./closing.ts";
import { forgetTranscript, offScreen, takeHeldSteers, transcripts } from "./session-follow.ts";
import { prependDraft } from "../chat/compose.tsx";
import { forgetLog } from "./session.tsx";

/// Stops the turn of `s`, its subagents, background commands and the agents it started; asks
/// first while any of those run.
function stopAll(s: AgentRow) {
  const o = openOf(s);
  const open = sameSession(AG.open, s) && !!AG.page;
  const run = async () => {
    // Another session may have opened while the sheet asked.
    if (sameSession(AG.open, s) && AG.page) return stopAllWork(s);
    // What waits on this device comes out first, so none of it goes after the stop.
    const kept = await drainOutbox(o);
    await invoke("agent_steer", { ...where(openOf(s)), steer: "stop_all" });
    // The node drops the steers it holds: they wait in the session's field instead, then what waited here.
    const steers = [...takeHeldSteers(o), ...kept.map(draft => draft.typed).filter(Boolean)];
    if (steers.length) prependDraft(conversationKey(s), steers.join("\n"));
  };
  const work: SideWork = {
    // The node's count reaches sessions that are not open; the open log may show one sooner.
    subagents: Math.max((open ? AG.page : transcripts.peek(transcriptKey(o))?.page)?.session.subagents ?? s.subagents ?? 0, open ? workingSubagents() : 0),
    tasks: (open ? AG.tasks : offScreen.get(transcriptKey(o))?.tasks ?? []).filter(runs).length,
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
    const items: MenuItem[] = [{ label: "Open", on: () => open(s) }];
    if (!onPhone() && S.view === "agent" && AG.open && !sameSession(AG.open, s)) {
      items.push({ label: "Open beside", on: () => void import("../agentpane/media.ts").then(m => m.openSessionBeside({
        machine_id: s.machine_id, machine_name: s.machine_name, agent: s.agent, id: s.id,
        title: s.title ?? null, cwd: s.cwd, status: s.status, external: s.external,
      })) });
    }
    const parent = parentSession(s);
    if (parent) items.push({ label: "Open parent", on: () => open(parent) });
    if (!s.external && s.assistant && !s.reviewer) {
      if (!onPhone()) items.push({ label: NEW_WINDOW, on: () => void import("../center/sidebar.tsx").then(({ openChatWindow }) => openChatWindow(s)) });
      items.push({ label: "New thread", on: () => { draftNewThread(s); open(s); } });
      const folder = chatFolder(s.cwd);
      if (folder) items.push({ label: "Skills", on: () => void import("../project/project-skills.tsx").then(({ openChatSkills }) => openChatSkills(folder, s.title || "this chat")) });
    }
    if (offers.stop) {
      items.push({ label: "Stop turn", on: () => void invoke("agent_steer", { ...at, steer: "cancel" }).catch(err => console.warn("stop turn:", err)) });
      items.push({ label: "Stop all", on: () => stopAll(s) });
    }

    if (offers.resume) {
      items.push({ label: "Resume", on: () => void resumeEndedSession(openOf(s), null).then(started => open(started.session), err => console.warn("resume:", err)) });
    }

    if (folder && !onPhone()) {
      items.push({ label: "Open in editor", on: () => void openInEditor(folder) });
      items.push({ label: "Open folder", on: () => void invoke("open_folder", { path: folder }).catch(err => console.warn("open folder:", err)) });
    }

    if (offers.delete) {
      const targets = windowHooks?.sessionDestinations?.(s) ?? [];
      if (targets.length) items.push({ label: "Move to project", on: () => {
        openMenu(e, "Move to project", targets.map(target => ({ label: target.name, on: () => confirmSheet({
          title: `Move to ${target.name}?`,
          text: "Keeps the conversation and changes its working folder. Files stay where they are. Resume the session after moving it.",
          action: "Move",
          run: async () => {
            await invoke("agent_relocate", { ...at, folder: target.folder });
            await loadSessions();
          },
        }) })));
      } });
    }

    items.push({ label: "Copy id", on: () => void writeClipboard(s.id) });

    if (offers.close) {
      items.push({
        label: "Close session",
        warn: true,
        on: () => void closeSession(s),
      });
    }

    if (offers.delete) {
      items.push({
        label: "Delete",
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
            await loadSessions().catch(err => console.warn("agent sessions:", err));
          },
        }),
      });
    }

    return items;
  };
  // Only a folder this Mac has can be opened here.
  const folder = invoke<string | null>("session_folder_here", { machineId: s.machine_id, cwd: s.cwd }).catch(() => null);
  const reachable = invoke<boolean>("agent_deletable", { machineId: s.machine_id }).catch(() => false);
  // Shown at once with what is known; the folder and the mesh fill in a moment later.
  const update = openMenu(e, s.title || "Session", build(null, false));
  update(build(await folder, await reachable));
}
