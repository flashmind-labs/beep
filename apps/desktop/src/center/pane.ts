// The bar's page as a pane in the window: its header, and handoffs from
// other windows.
import { createElement } from "react";
import { invoke, listen } from "../ui/tauri.ts";
import { pane } from "../bar/pane.ts";
import type { View } from "../bar/core.ts";
import type { CenterTarget } from "../project/projectwindow.ts";
import { chatProject, shortName, type ProjectAgent } from "../project/agent.ts";
import { askBeep } from "./center.ts";
import { type ViewHead, state } from "./state.ts";
import { PROJECT_COLORS } from "../ui/icons.ts";
import { startedWhen } from "./home.ts";
import { currentLocation, go, drawTop, openResearch } from "./views.ts";
import { paneOn, chatList, projectRows, openProjects } from "./sidebar.ts";
import { loadThreadChats, threadHead, pinButton, threadChats } from "./threadcard.ts";
import { ensureHome } from "./refresh.ts";
import { showNotices } from "./notices.ts";
import { openReport } from "../report/report.ts";
import { debugPrompt } from "../report/report-state.ts";
import type { MacControl } from "../machines/machine-state.ts";

const ROUTINES_TITLE: [string, string] = ["Routines", "What Beep does on its own"];
const APPS_TITLE: [string, string] = ["Apps", "What Beep can use for you"];
const SETTINGS_TITLE: [string, string] = ["Settings", ""];

/// Titles of the pane's views; a chat or group takes its own name.
const PANE_TITLES: Partial<Record<View, [string, string]>> = {
  chats: ["Chats", "Every chat, from the bar, this window and your phone"],
  memories: ["Memories", "What Beep knows about you"],
  routines: ROUTINES_TITLE,
  rnew: ROUTINES_TITLE,
  grants: ROUTINES_TITLE,
  skills: ["Skills", "What Beep knows how to do"],
  apps: APPS_TITLE,
  mcp: APPS_TITLE,
  appperms: APPS_TITLE,
  settings: SETTINGS_TITLE,
  machines: SETTINGS_TITLE,
  research: ["Research", ""],
  agents: ["Coding agents", "On your machines"],
  agentnew: ["Start a coding agent", "It runs on your machine; follow it and answer it here"],
  agent: ["Coding agents", "Follow it and answer it here"],
};

export function paneTitle(): ViewHead {
  const on = paneOn();

  if (on.view === "chat") {
    const c = chatList.find(x => x.id === on.session);
    // A chat just started is not in the list until the server has it.
    const at = pane.startedAt();
    const thread = on.session ? threadChats?.get(on.session) : undefined;
    if (thread) return threadHead(thread, c);
    return { title: c?.title ?? "New chat", sub: at ? `Started ${startedWhen(at)}` : "", reactTools: c ? pinButton(c) : null, reactScope: scopeTag(on.session) };
  }

  if (on.group) {
    const g = chatList.find(x => x.id === on.group);
    return { title: g?.title ?? "Group", reactTools: g ? pinButton(g) : null };
  }

  const [title, sub] = (on.view ? PANE_TITLES[on.view] : null) ?? ["Beep", ""];
  if (on.view === "agentnew") return { title, sub, reactTools: createElement("button", { type: "button", className: "dbtn small", onClick: () => go("pane", "agents") }, "All sessions") };
  return { title, sub };
}

/// Beside a chat's title: the project it belongs to, which opens the
/// project, or General.
export function scopeTag(session: string | null) {
  // A chat about one thread runs as its project's agent too.
  const thread = session ? threadChats?.get(session) : undefined;
  const agent = chatProject(state.agents, session) ?? (thread ? { name: thread.project } : null);
  if (!agent) return createElement("span", { className: "chip" }, "General");
  const i = (projectRows() || []).findIndex(p => p.name === agent.name);
  return createElement("button", { type: "button", className: "chip pj-scope", title: `${agent.name}: open the project`, onClick: () => openProjects(agent.name) },
    createElement("span", { className: "cc-pdot", style: i >= 0 ? { background: PROJECT_COLORS[i % PROJECT_COLORS.length] } : undefined }), shortName(agent.name));
}

/// The project agents alone, so a chat's header knows its project before
/// any project page has loaded them.
export async function loadAgents() {
  if (!state.subscribed) {
    state.agents = null;
    return;
  }

  try {
    state.agents = await invoke<ProjectAgent[] | null>("center_projects");
  } catch (e) {
    console.warn("project agents:", e);
    return;
  }

  if (currentLocation()?.view === "pane") drawTop();
  loadThreadChats();
}

// ---------- Handoffs ----------

/// The Report a problem sheet's "Debug with an agent": the start form on this
/// Mac, when Beep may use it, with a prompt to look into Beep and file a report.
export async function debugWithAgent(text: string) {
  const mac = state.mac ?? await invoke<MacControl>("mac_control").catch(() => null);
  go("pane", { agentnew: { prompt: debugPrompt(text), at: mac?.enabled ? { machine: mac.id } : null } });
}

/// Shows what the bar or tray asked for, once; Home when nothing is waiting.
export async function takePending(fallback: "home" | null) {
  const target = await invoke<CenterTarget | null>("center_pending").catch(() => null);
  if (target == null) return fallback && go(fallback);
  if (target === "home" || target === "instructions" || target === "providers" || target === "terminals" || target === "research") return go(target);

  // The report target: the Report a problem sheet over the page on screen.
  if (target === "report") {
    if (!currentLocation()) go("home");
    return openReport(document.body, debugWithAgent);
  }

  if (typeof target === "object" && "projects" in target) return go("projects", target.projects);
  if (typeof target === "object" && "ask" in target) return askBeep(target.ask.prompt);
  if (typeof target === "object" && "open_research" in target) return openResearch(target.open_research.session);

  // A notification's click: the page behind stays, the pane opens over it.
  if (typeof target === "object" && "notice" in target) {
    if (!currentLocation()) go("home");
    return showNotices(target.notice.session);
  }

  go("pane", target);
  // Home is not on screen, but the sidebar counts come from it. After the
  // first load its topics keep them current.
  ensureHome();
}

listen("center-open", () => takePending(null));
