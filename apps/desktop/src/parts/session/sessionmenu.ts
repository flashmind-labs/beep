import { S, windowRequests } from "../../stores/bar/core.ts";
import { sessionDestinations, sessionProject } from "../project/sessionproject.ts";
import { readProjectHarnesses } from "../project/project-harnesses.tsx";
import { forgetView } from "../../stores/agentpane/view-memory.ts";
import { onPhone, WINDOWS } from "../../ui/platform.ts";
import { NEW_WINDOW } from "../../stores/project/projectwindow.ts";
// The right-click menu of a coding agent session, shared by every row that shows one.
import { confirmSheet, openMenu, type MenuItem, type MenuLine } from "../../ui/menu.tsx";
import { writeClipboard } from "../../ui/clipboard.ts";
import { invoke } from "../../ui/tauri.ts";
import { openInEditor } from "../project/editors.ts";
import { chatFolder, sessionHref } from "../../stores/agents/nesting-state.ts";
import type { AgentRow } from "../../stores/agents/agents-state.ts";
import { parentSession, sessions } from "../../stores/agents/sessions.ts";
import { type SideWork, sessionOffers, stopAllAsks } from "../../stores/agents/sessionmenu-state.ts";
import { AG, AGENT_ACTIVE, ensureAgentHosts, openOf, where } from "../../stores/agents/agents-state.ts";
import { sameSession } from "../../stores/agents/running-state.ts";
import { canContinueElsewhere } from "../../stores/agents/continue-state.ts";
import { canContinueWith, harnessesFor } from "./modelpill.tsx";
import { agentKind } from "../../stores/agents/agent-kind.ts";
import { effortsOf, modelId, modelRows } from "../../stores/agents/agentpick.ts";
import { knownModels, modelsOn } from "../../stores/agents/model-store.ts";
import { drawAgentBody, workingSubagents } from "./logview.tsx";
import { type Draft, continueSessionNow, draftNewThread, drainOutbox, drawQueue, heldQueue, restoreDraft, resumeEndedSession, sendOnce } from "./messages.ts";
import { runs, transcriptKey, transcripts } from "../../stores/agents/transcript-cache-state.ts";
import { closeSession } from "../../stores/agents/closing.ts";
import { clearHeldSteers, forgetTranscript, heldSteers } from "./session-follow.ts";
import { continuePicks, forgetLog, resumeFollow, setNote, turnRuns } from "./open-state.ts";
import { conversationKey } from "../../stores/agents/conversation-state.ts";
import { currentMentions, mentionsOf } from "../../stores/agents/mentions.ts";
import { heldMessage } from "../../stores/agents/agentqueue.ts";
import { openChatWindow } from "../project/projectops.ts";
import { redrawAgentHints } from "./session-head.tsx";

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

/// Walks `Continue with` in the menu: the harness, its model, then its thinking
/// level when it has levels. The last pick starts the continued session, which opens.
function continueSteps(e: MouseEvent, s: AgentRow, open: (s: AgentRow) => void) {
  const back = (to: () => void): MenuItem[] => [{ label: "Back", icon: "back", on: to }, { kind: "separator" }];
  const start = (agent: string, model: string, effort: string | null) => {
    continuePicks.set(s.id, { agent, model: modelId(knownModels(s.machine_id, agent), model), effort, fast: null });
    if (!sameSession(AG.open, s)) open(s);
    setTimeout(() => void continueSessionNow(openOf(s)), 0);
  };

  const harnesses = () => openMenu(e, "Continue with", [
    ...back(() => void sessionMenu(e, s, open)),
    ...harnessesFor(s).filter(agent => agent !== s.agent).map(agent => ({ label: agentKind(agent).name, chevron: true, on: () => models(agent) })),
  ]);

  const models = (agent: string) => {
    const thinks = effortsOf(agent).length > 0;
    const lines = (): MenuItem[] => {
      const { rows, note } = modelRows(knownModels(s.machine_id, agent), "");
      return [
        ...back(harnesses),
        ...rows.map(row => ({ label: row.label, detail: row.note || undefined, chevron: thinks, on: () => thinking(agent, row.value) })),
        ...(note ? [{ label: note, disabled: true, on: () => {} }] : []),
      ];
    };
    const update = openMenu(e, agentKind(agent).name, lines(), "Model");
    modelsOn(s.machine_id, agent, () => update(lines()));
  };

  const thinking = (agent: string, model: string) => {
    const efforts = effortsOf(agent);
    if (!efforts.length) return start(agent, model, null);
    openMenu(e, agentKind(agent).name, [
      ...back(() => models(agent)),
      { label: "Default", on: () => start(agent, model, null) },
      ...efforts.map(effort => ({ label: effort.label, on: () => start(agent, model, effort.value) })),
    ], "Thinking");
  };

  harnesses();
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
      openGroup.push({ label: "Open beside", icon: "splitPane", on: () => void import("./media.ts").then(m => m.openSessionBeside({
        machine_id: s.machine_id, machine_name: s.machine_name, agent: s.agent, id: s.id,
        title: s.title ?? null, cwd: s.cwd, status: s.status, external: s.external,
      })) });
    }
    const parent = parentSession(s);
    if (parent) openGroup.push({ label: "Open parent", icon: "back", on: () => open(parent) });
    if (!s.external && s.assistant && !s.reviewer) {
      if (!onPhone()) openGroup.push({ label: NEW_WINDOW, icon: "openWindow", on: () => openChatWindow(s) });
      openGroup.push({ label: "New thread", icon: "chat", on: () => { draftNewThread(s); open(s); } });
      const chatDir = chatFolder(s.cwd);
      if (chatDir) openGroup.push({ label: "Skills", icon: "skill", chevron: true, on: () => windowRequests.notify({ kind: "chat-skills", folder: chatDir, title: s.title || "this chat" }) });
    }

    const runGroup: MenuLine[] = [];
    if (offers.stop) {
      runGroup.push({ label: "Stop turn", icon: "stop", detail: onPhone() ? undefined : (WINDOWS ? "Ctrl+." : "⌘."), on: () => void invoke("agent_steer", { ...at, steer: "cancel" }).catch(err => console.warn("stop turn:", err)) });
      runGroup.push({ label: "Stop all", icon: "stopAll", on: () => stopAll(s) });
    }
    if (canContinueElsewhere(s) && canContinueWith(s)) {
      runGroup.push({ label: "Continue with", icon: "sync", chevron: true, on: () => continueSteps(e, s, open) });
    }
    if (offers.resume) {
      runGroup.push({ label: "Resume", icon: "run", on: () => resumeEndedSession(openOf(s), started => open(started)) });
    }

    const workspaceGroup: MenuLine[] = [];
    if (folder && !onPhone()) {
      workspaceGroup.push({ label: "Open in editor", icon: "paneText", on: () => void openInEditor(folder) });
      workspaceGroup.push({ label: "Open folder", icon: "folder", on: () => void invoke("open_folder", { path: folder }).catch(err => console.warn("open folder:", err)) });
    }
    if (offers.delete) {
      const targets = sessionDestinations(s);
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
    workspaceGroup.push({ label: "Copy link", icon: "copy", on: () => void writeClipboard(sessionHref(s)) });

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
  const hosts = AG.hosts ? Promise.resolve() : ensureAgentHosts().then(() => {});
  const project = sessionProject(s)?.name ?? null;
  const harnesses = project ? readProjectHarnesses(project) : Promise.resolve();
  // Shown at once with what is known; the folder, hosts and mesh fill in a moment later.
  const update = openMenu(e, s.title || "Session", build(null, false), s.machine_name || undefined);
  const [resolvedFolder, resolvedReachable] = await Promise.all([folder, reachable, hosts, harnesses]);
  update(build(resolvedFolder, resolvedReachable));
}

type StopAllPhase = { kind: "draining" } | { kind: "stopping"; drafts: Draft[] };
const stoppingSessions = new Map<string, StopAllPhase>();

/// Stops the session and its side work, returning the pending messages to its draft.
export async function stopAllWork(target: Pick<AgentRow, "machine_id" | "agent" | "id">) {
  const key = conversationKey(target), o = openOf(target);
  if (stoppingSessions.has(key)) return;

  const showing = () => S.view === "agent" && sameSession(AG.open, target);
  const mentions = showing() ? currentMentions() : mentionsOf(target);
  stoppingSessions.set(key, { kind: "draining" });
  let stoppedSteers: Draft[] = [];

  try {
    const drafts = await drainOutbox(o);
    stoppingSessions.set(key, { kind: "stopping", drafts });

    const stopping = showing() && turnRuns() ? { id: target.id, turns: AG.page?.turns ?? 0 } : null;

    if (stopping) {
      AG.stopping = stopping;
      drawQueue();
      redrawAgentHints();
      drawAgentBody();
    }

    const failed = await sendOnce(o, { stop_all: null }, null);
    if (!failed) {
      // What the node still listed when it answered: a steer it delivered meanwhile is not taken back.
      const held = showing() ? heldQueue() : heldSteers(o);
      clearHeldSteers(o);
      stoppedSteers = held.map(text => ({ typed: heldMessage(text).words, paths: [], images: [] }));
    }

    if (showing()) {
      if (failed && AG.stopping === stopping) AG.stopping = null;
      setNote(failed?.message ?? "");
    } else if (failed) {
      throw new Error(failed.message);
    }
  } finally {
    const phase = stoppingSessions.get(key);
    stoppingSessions.delete(key);

    if (phase?.kind === "stopping") {
      const state = showing() ? currentMentions() : mentions;
      // Prepend newest first so the field keeps the original order.
      for (const q of [...stoppedSteers, ...phase.drafts].reverse()) restoreDraft(q, state, o);
    }

    if (showing()) {
      drawQueue();
      redrawAgentHints();
      drawAgentBody();
      resumeFollow();
    }
  }
}
