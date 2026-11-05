import { loadAgentHosts } from "../agents/agents.ts";
import { renderAgentNew } from "../agents/agentnew.ts";
import { loadApps, renderApps } from "../settings/apps.ts";
import { focusAsk, newChat, renderChat } from "../chat/chat.ts";
import { renderChats } from "../chat/chats.ts";
import { $, bar, GROUP_VIEWS, type LinkState, PANE, S } from "./core.ts";
import { loadGroup } from "../chat/groups.ts";
import { openSession } from "../chat/history.ts";
import { renderLink, renderRevoked } from "./link.ts";
import { drawDock, loadLiveChats, LV, openLive, renderLive } from "./live.ts";
import { renderMemories } from "../lists/memories.ts";
import { loadModels } from "../settings/model.ts";
import { loadResearches, renderResearches } from "../research/researches.ts";
import { renderRoutines } from "../lists/routines.ts";
import { renderSettings } from "../settings/settings.ts";
import { renderSkills } from "../lists/skills.ts";
import { invoke } from "../ui/tauri.ts";

// ---------- State ----------

/// Whether this Mac is linked, and the chat Rust has current (Status).
export interface Status {
  link: LinkState;
  session: string | null;
}

export async function refreshStatus() {
  const st = await invoke<Status>("status");
  S.link = st.link;
  // The bar's chat is not the window's.
  if (S.session === null && !PANE) S.session = st.session;
  return st;
}

/// Views the bar shows by name when Rust names one, once linked.
const NAMED: Record<string, () => unknown> = {
  new: newChat,
  chats: renderChats,
  memories: renderMemories,
  routines: renderRoutines,
  skills: renderSkills,
  research: renderResearches,
  apps: renderApps,
};

/// Shows the bar on `view`, as Rust names it, or on what it showed last.
export async function show(view: string | null) {
  const st = await refreshStatus();
  loadResearches();
  loadLiveChats();
  if (S.link === "linked") { loadModels(); loadAgentHosts(); }
  if (S.link === "revoked") return renderRevoked();
  if (view === "settings") return renderSettings();
  if (view === "link") return S.link === "linked" ? renderChat() : renderLink();
  // Without a subscription the bar starts agents on the mesh's machines; the chat needs one.
  if (S.link === "unlinked") return renderAgentNew("", "chat");
  if (view && Object.hasOwn(NAMED, view)) return NAMED[view]();
  // Back from the browser: look again at what is connected.
  if (S.view === "apps") return loadApps();
  // A chat picked from the tray's Recent menu, or the orb's approval card,
  // already made current in Rust. In live mode it opens under the dock.
  if (view === "open" && st.session) {
    if (!LV.on) return openSession(st.session);
    if (S.view !== "chat" || !$("#ldock") || LV.chat !== st.session) openLive(st.session);
    return drawDock(LV.view);
  }
  // A group stays up between showings, fetched again for what came in.
  if (GROUP_VIEWS.has(S.view)) {
    if (S.view === "group") { loadGroup(); $("#gask")?.focus(); }
    else if (S.view === "ginfo") loadGroup();
    else bar.querySelector("input")?.focus();
    return;
  }
  // Live mode runs in the bar until it ends.
  if (LV.on) return renderLive();
  if (S.view !== "chat" || !$("#ask")) {
    if (S.session) return openSession(S.session);
    return renderChat();
  }
  focusAsk();
}
