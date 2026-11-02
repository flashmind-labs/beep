import { loadAgentHosts } from "./agents.ts";
import { loadApps, renderApps } from "./apps.ts";
import { focusAsk, newChat, renderChat } from "./chat.ts";
import { renderChats } from "./chats.ts";
import { $, bar, GROUP_VIEWS, invoke, PANE, S } from "./core.ts";
import { loadGroup } from "./groups.ts";
import { openSession } from "./history.ts";
import { renderLink, renderRevoked } from "./link.ts";
import { drawDock, loadLiveChats, LV, openLive, renderLive } from "./live.ts";
import { renderMemories } from "./memories.ts";
import { loadModels } from "./model.ts";
import { loadResearches, renderResearches } from "./researches.ts";
import { renderRoutines } from "./routines.ts";
import { renderSettings } from "./settings.ts";
import { renderSkills } from "./skills.ts";

// ---------- State ----------

export async function refreshStatus() {
  const st = await invoke("status");
  S.link = st.link;
  // The bar's chat is not the window's.
  if (S.session === null && !PANE) S.session = st.session;
  return st;
}

export async function show(view) {
  const st = await refreshStatus();
  loadResearches();
  loadLiveChats();
  if (S.link === "linked") { loadModels(); loadAgentHosts(); }
  if (S.link === "revoked") return renderRevoked();
  if (view === "settings") return renderSettings();
  if (S.link === "unlinked" || view === "link") return S.link === "linked" ? renderChat() : renderLink();
  if (view === "new") return newChat();
  if (view === "chats") return renderChats();
  if (view === "memories") return renderMemories();
  if (view === "routines") return renderRoutines();
  if (view === "skills") return renderSkills();
  if (view === "research") return renderResearches();
  if (view === "apps") return renderApps();
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

