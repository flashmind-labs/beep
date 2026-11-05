// The pane: the bar's views drawn inside the command center window, which
// drives them through `pane`.
import "./events.ts";
import { type AgentPlace, renderAgentNew } from "../agents/agentnew.ts";
import { type AgentRow, loadAgentHosts, renderAgents } from "../agents/agents.ts";
import { openAgent } from "../agents/session.ts";
import { renderApps } from "../settings/apps.ts";
import { body, focusAsk, grow, newChat, renderChat, send } from "../chat/chat.ts";
import { renderChats, rows, type SessionSummary } from "../chat/chats.ts";
import { $, type BackTo, changeListeners, type ChatRow, connectWindow, current, G, GROUP_VIEWS, live, type PaneNow, S } from "./core.ts";
import { type GroupSummary, renderGroup } from "../chat/groups.ts";
import { refreshHints } from "./hints.ts";
import { drawChatTop, openSession } from "../chat/history.ts";
import { mountPane } from "./keys.ts";
import { renderInvitePhone, renderLink, renderRevoked } from "./link.ts";
import { renderMachines } from "../machines/machines.ts";
import { renderMemories } from "../lists/memories.ts";
import { loadModels } from "../settings/model.ts";
import { loadResearches, RESEARCHES } from "../research/researches.ts";
import { renderRoutines, routineWhen } from "../lists/routines.ts";
import { renderSettings, settingsBack } from "../settings/settings.ts";
import { renderSkills } from "../lists/skills.ts";
import type { Status } from "./views.ts";
import { invoke } from "../ui/tauri.ts";

/// What the window can open in the pane: a CenterTarget in its JSON form,
/// a group, or an agent view.
export type PaneTarget =
  | string
  | null
  | { chat: { session: string | null } }
  | { group: { id: string } }
  | { agent: AgentRow & { kind?: string }; from?: BackTo }
  | { agentnew: { prompt?: string; at?: AgentPlace | null }; from?: BackTo };

/// Pages the window opens in the pane by name.
const VIEWS: Record<string, () => unknown> = {
  chats: renderChats,
  memories: renderMemories,
  routines: renderRoutines,
  skills: renderSkills,
  apps: renderApps,
  settings: renderSettings,
  // Pages Settings leads to, which the window shows in the pane.
  machines: renderMachines,
  link: renderLink,
  invite: renderInvitePhone,
  // The command center leads here when the server says this Mac was removed.
  revoked: renderRevoked,
};

let started: Promise<void> | null = null;
/// Once per page: what the bar loads each time it shows, less live mode.
const start = () => started ??= refreshPaneStatus().catch(() => {});
async function refreshPaneStatus() {
  const st = await invoke<Status>("status");
  S.link = st.link;
  const researches = loadResearches();
  if (S.link === "linked") { loadModels(); loadAgentHosts(); }
  await researches;
}

/// Back on the chat or group the pane still shows, made current again
/// after `leave`.
function backOn() {
  if (S.view === "group") return renderGroup(G.id!);
  if (S.session) invoke("open_session", { id: S.session }).catch(() => {});
  focusAsk();
}

export const pane = {
  /// A routine's schedule in words, as Routines shows it.
  when: routineWhen,
  /// Hands the pane what the window does for it.
  connect: connectWindow,
  /// Draws Settings into `el`, the window's sheet, leaving the pane as it
  /// is. It opens on `page` when given, else on the page picked last.
  async settings(el: HTMLElement, page?: string) {
    await start();
    return renderSettings(false, el, page);
  },
  /// Esc in the sheet: back from a subpage to its page. False when the
  /// sheet shows a page, so Esc closes it.
  settingsBack,
  /// Shows `target`.
  async open(target: PaneTarget) {
    await start();
    if (target && typeof target === "object") {
      if ("chat" in target) {
        const id = target.chat?.session ?? null;
        if (!id) return newChat();
        if (id === S.session && S.view === "chat" && $("#ask")) return backOn();
        return openSession(id);
      }
      // Groups are not a CenterTarget: only the window's sidebar opens them.
      if ("group" in target) return renderGroup(target.group.id);
      // Agent views are not a CenterTarget either: Home and the sidebar open them.
      if ("agent" in target) return openAgent(target.agent, { back: target.from ?? "agents" });
      if ("agentnew" in target) return renderAgentNew(target.agentnew.prompt ?? "", target.from ?? "agents", target.agentnew.at ?? null);
    }
    if (target === "agents") return renderAgents();
    if (typeof target === "string" && VIEWS[target]) return VIEWS[target]();
    if ((S.view === "chat" && $("#ask")) || (S.view === "group" && G.id)) return backOn();
    return S.session ? openSession(S.session) : renderChat();
  },
  /// The pane goes off screen: the window stops showing its chat or group,
  /// so their notices show again.
  leave() {
    if (S.view === "chat") invoke("new_chat").catch(() => {});
    else if (GROUP_VIEWS.has(S.view)) invoke("close_group").catch(() => {});
  },
  /// Starts a new chat with `text` and streams it here.
  async compose(text: string) {
    await start();
    await newChat();
    return send(text);
  },
  /// Opens chat `session` with its history and sends `text` in it,
  /// streaming the reply here.
  async ask(session: string, text: string) {
    await start();
    await openSession(session);
    if (S.session !== session || S.view !== "chat") return;
    return send(text);
  },
  /// Starts a new chat with `text` in the composer, not sent, for the
  /// user to send or change.
  async draft(text: string) {
    await start();
    await newChat();
    const ask = $<HTMLTextAreaElement>("#ask");
    if (!ask) return;
    ask.value = text;
    grow(ask);
    refreshHints();
    focusAsk();
  },
  /// Chats and groups, newest first, pinned ones as the bar lists them.
  async chatRows(): Promise<(ChatRow & { research: boolean })[]> {
    // The research set loads with the pane, so rows know which open their own window.
    await start();
    const [sessions, groups] = await Promise.all([
      invoke<SessionSummary[]>("list_sessions"),
      invoke<GroupSummary[]>("list_groups").catch((): GroupSummary[] => []),
    ]);
    return rows(sessions, groups).map(c => ({ ...c, research: RESEARCHES.has(c.id) }));
  },
  /// The chats a reply streams into here.
  running(): string[] {
    return [...live].filter(([id, e]) => id && e.run).map(([id]) => id!);
  },
  /// The chats with a run going on, started from any device; empty from a
  /// server too old to say.
  activeRuns(): Promise<string[]> {
    return invoke<string[]>("active_runs").catch((): string[] => []);
  },
  /// Draws what the window shows atop the open chat again, once it knows.
  chatTop: drawChatTop,
  /// When the open chat's first message was sent, in milliseconds, or
  /// null before its history has loaded.
  startedAt(): number | null {
    if (S.view !== "chat") return null;
    const at = Number(body()?.querySelector<HTMLElement>("[data-at]")?.dataset.at);
    return at > 0 ? at : null;
  },
  current,
  onChange(fn: (now: PaneNow) => void) { changeListeners.push(fn); },
  /// Whether the pane is on screen and takes the keys.
  mounted(on: boolean) {
    mountPane(on);
    if (on && S.view === "chat") focusAsk();
  },
};
