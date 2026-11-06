// center/: the command center window: sidebar, Home, palette, usage pill and
// the window's shared state (state.ts). Entry: this file (dist/center.js).
import "../ui/loopclock.ts";
import { closeFocusedTab } from "../agentpane/media.ts";
import { isCloseKey } from "../agentpane/media-state.ts";
import { isNewKey } from "../terminals/newkey-state.ts";
import { allTerms } from "../terminals/view.ts";
import { invoke, listen } from "../ui/tauri.ts";
import { initPalette, openPalette } from "./palette.ts";
import { initSideResize } from "./sidewidth.ts";
import { pane } from "../bar/pane.ts";
import { type BackTo, S } from "../bar/core.ts";
import { seed, type CachedViews } from "./cached.ts";
import type { HomeView } from "./home-state.ts";
import { el as h } from "../ui/dom.ts";
import { startView, windowTitle } from "../project/projectwindow.ts";
import { shortName, newChatTarget } from "../project/agent.ts";
import { mountUsage, type UsageView } from "./usage.ts";
import { mountNotices } from "./notices.ts";
import { firstRun } from "./firstrun.ts";
import { viewHidden, keyHidden } from "../settings/subscription.ts";
import { windowTakes } from "../ui/windowkeys.ts";
import { RESEARCHES } from "../research/researches.ts";
import { type ViewArg, $, state, pagesAccount, keepPagesFile } from "./state.ts";
import { dismissedUpdates, draw, jump } from "./home.ts";
import { shown, shownArg, windowProject, toMain, go, goHistory, setWindowProject, openResearch } from "./views.ts";
import { reloadGroup, paneOn, reloadChats, openProjects, openProjectWindow, addProject } from "./sidebar.ts";
import "../research/page.ts";
import "../research/open.ts";
import { escUp } from "./escup-state.ts";
import { openTile } from "../project/split.ts";
import { pa } from "../project/sections.ts";
import { AG } from "../agents/agents.ts";
import { loadAgents, takePending } from "./pane.ts";
import { threadCard, threadChats } from "./threadcard.ts";
import { openSettings, embedView, closeSettings } from "../settings/sheet.ts";
import { followSkin } from "../ui/skin.ts";
import { guardClicks } from "../ui/clickguard.ts";
import { reason } from "../ui/format.ts";
import type { MacControl } from "../machines/machine-state.ts";
import { projectOf, repoOf } from "../agents/running-state.ts";

followSkin();
guardClicks();

// The web view's own menu only offers Reload, which restarts the whole UI. Items with a
// menu of their own stop the event first; here it stays native only where text can be
// copied or pasted.
document.addEventListener("contextmenu", e => {
  const on = e.target instanceof Element ? e.target : null;
  if (on?.closest("input, textarea, [contenteditable], .tm-xterm")) return;
  if (getSelection()?.toString()) return;
  e.preventDefault();
});

// ---------- Keys ----------

initSideResize($(".cwin"), $("#side-grip"));
mountUsage($("#usage"), refresh => invoke<UsageView | null>("usage_get", { refresh }));
mountNotices($("#bell"));

initPalette({ go, jump, ask: askBeep, scope: () => windowProject, openChat: id => (RESEARCHES.has(id) ? openResearch(id) : go("pane", { chat: { session: id } })), research: id => RESEARCHES.has(id), settings: openSettings, providers: () => go("providers"), project: openProjects, addProject, branches: name => { if (openProjects(name)) openTile("branches"); } });

/// ⌘ and a letter: the pane page it opens.
const PANE_KEYS: Record<string, string> = { m: "memories", j: "routines", l: "skills" };

document.addEventListener("keydown", e => {
  // Keys typed into a terminal are the terminal's, Esc and Tab too, but not ⌘W or ⌘N.
  if (e.target instanceof Element && e.target.closest(".tm-xterm") && !isCloseKey(e) && !isNewKey(e)) return;
  // Esc goes up one page: a project's open tile closes, then the Projects page, then Home.
  if (e.key === "Escape" && !e.defaultPrevented) {
    const up = escUp({ page: shown, picked: state.picked, tile: !!pa.tile, alone: !!windowProject });
    if (up) {
      e.preventDefault();
      if (up.to === "tile") return openTile(null);
      return up.to === "home" ? go("home") : void openProjects();
    }
  }
  if (!e.metaKey) return;
  const key = e.key.toLowerCase();
  if ((key === "[" || key === "]") && !e.shiftKey) {
    e.preventDefault();
    return goHistory(key === "]");
  }
  // ⌘W closes the focused pane's tab, else this window; Beep keeps running in the menu bar. The main
  // window only hides, keeping what it shows.
  if (isCloseKey(e)) { e.preventDefault(); if (closeFocusedTab()) return; invoke("center_close_window").catch(() => window.close()); return; }
  // ⌘⇧N: the project on screen in a window of its own.
  if (key === "n" && e.shiftKey && shown === "projects" && state.picked && !windowProject) {
    e.preventDefault();
    return openProjectWindow(state.picked);
  }
  // Without a subscription the assistant's pages are not there. ⌘K still searches.
  if (key !== "k" && keyHidden(key, state.subscribed)) { e.preventDefault(); return; }
  // ⌘M on the pane's start form and ⌘R in its chats list are the pane's.
  if (windowTakes(key, shown === "pane" ? paneOn().view : undefined)) {
    e.preventDefault();
    if (key === "k") return openPalette();
    if (key === ",") return openSettings();
    // ⌘R opens the window's Research page, not the bar's list.
    if (key === "r") return go("research");
    const to = PANE_KEYS[key];
    if (to) return go("pane", to);
  }
  // In the pane, the pane's own keys call newAgentHere. On the Terminals page with
  // a terminal open, a new terminal like it.
  if (key === "n" && shown !== "pane") {
    e.preventDefault();
    if (shown === "terminals" && isNewKey(e) && allTerms.newLikeOpen()) return;
    return newAgentHere();
  }
  // ⌘T in a project: a terminal filling the empty view. In a session the pane has taken it.
  if (key === "t" && !e.shiftKey && !e.altKey && !e.ctrlKey && !e.defaultPrevented) {
    const to = projectHere();
    if (to) { e.preventDefault(); go("pane", { draft: { project: to.project, machine: to.machine, terminal: true } }); }
  }
});

/// The machines online in `home`, as a key that changes when one connects.
export function onlineMachines(home: HomeView | null) {
  return (home?.machines || []).filter(m => m.online).map(m => m.id).sort().join(" ");
}

/// The project on screen, its page, a session or chat of it, and the machine
/// its start input opens on; null elsewhere.
function projectHere() {
  const on = paneOn();
  if (on.view === "agent" && AG.draft) return { project: AG.draft, machine: null };
  const open = shownArg && typeof shownArg === "object" && "agent" in shownArg ? shownArg.agent : null;
  return newChatTarget({ shown, picked: state.picked, view: on.view, session: on.session, agent: on.view === "agent" ? open : null, mac: state.mac?.id }, state.agents, state.home?.projects);
}

/// ⌘N: in a project (its page, or one of its sessions or chats) the agent
/// view with an empty conversation and the project's start input, focused;
/// elsewhere the start form.
function newAgentHere() {
  const to = projectHere();
  if (!to) return void go("pane", { agentnew: { prompt: "" } });
  go("pane", { draft: { project: to.project, machine: to.machine } });
}
pane.connect({
  openSettings,
  openInstructions() {
    closeSettings();
    go("instructions");
  },
  openProviders() {
    closeSettings();
    go("providers");
  },
  settingsAway(view) {
    closeSettings();
    if (view) go("pane", view);
  },
  embedView,
  newAgent: newAgentHere,
  goBack: (from: BackTo) => go(from.view, from.arg as ViewArg),
  chatTop(session) {
    const t = threadChats?.get(session);
    return t ? threadCard(t) : null;
  },
  sessionProject(s) {
    const projects = state.home?.projects ?? [], p = projectOf(s, projects);
    return p ? { label: repoOf(s, projects) || p.name, name: p.name, open: () => void openProjects(p.name) } : null;
  },
});

$<HTMLFormElement>("#home-comp").addEventListener("submit", async e => {
  e.preventDefault();
  const input = $<HTMLInputElement>("#home-ask"), note = $("#home-err"), prompt = input.value.trim();
  if (!prompt) return;
  note.textContent = "";
  input.value = "";
  go("pane", { chat: { session: null } });
  try {
    await pane.compose(prompt);
  } catch (err) {
    // Back on Home with the prompt as typed, where the error shows.
    input.value = prompt;
    go("home");
    note.textContent = reason(err);
  }
  reloadChats();
});

$("#home-agent")?.addEventListener("click", () => {
  const input = $<HTMLInputElement>("#home-ask"), prompt = input.value.trim();
  input.value = "";
  go("pane", { agentnew: { prompt } });
});

/// The first view.
export async function start() {
  const project = await invoke<string | null>("center_window_project").catch(() => null);
  document.title = windowTitle(project);
  const first = startView(project);
  if (!first) return takePending("home");
  // A project window: no sidebar, and a button back to the project.
  const name = first.arg.project;
  setWindowProject(name);
  document.body.classList.add("pw");
  const back = h("button", "dbtn small pw-back", shortName(name));
  back.type = "button";
  back.id = "pw-back";
  back.title = `Back to ${name} (esc)`;
  back.addEventListener("click", () => openProjects(name));
  $("#title").before(back);
  go(first.view, first.arg);
}

/// A new chat that starts with `prompt`, from the palette's ⌘↵ or another
/// window; a project window asks the main window.
export function askBeep(prompt: string) {
  if (windowProject) return toMain({ ask: { prompt } });
  go("pane", { chat: { session: null } });
  pane.compose(prompt).catch(e => console.warn("asking Beep:", e));
}

/// Shows what the app kept from the server's last answers, so the window
/// opens on Home, its projects and sidebar before the server answers.
async function showCached() {
  const [cached, mac, link] = await Promise.all([
    invoke<CachedViews | null>("center_cached").catch(e => { console.warn("kept views:", e); return null; }),
    invoke<MacControl>("mac_control").catch(() => null),
    invoke<{ link: string; signed_out: boolean; server: boolean }>("status").catch(() => null),
  ]);
  state.subscribed = link?.link === "linked";
  S.signedOut = link?.signed_out ?? false;
  S.server = link?.server ?? false;
  $("#home-comp").style.display = state.subscribed ? "" : "none";
  const home = seed(state, cached, dismissedUpdates);
  state.mac ??= mac;
  keepPagesFile(cached?.pages);
  pagesAccount(mac);
  if (!home) return;
  draw();
  reloadGroup("projects");
  reloadGroup("pinned");
  reloadGroup("machines");
}

// Signing in or out changes what the window shows.
listen<string>("link-state", e => {
  const now = e.payload === "linked";
  if (now === state.subscribed) return;
  state.subscribed = now;
  for (const name of ["pinned", "chats", "views", "research"]) reloadGroup(name);
  if (now) loadAgents();
  else {
    state.agents = null;
    // Signed out on a page that is only there with a subscription: Home.
    const on = paneOn();
    const arg = on.group ? { group: { id: on.group } } : on.session ? { chat: { session: on.session } } : on.view;
    if (shown && viewHidden(shown, arg, false, S.server)) go("home");
  }
  draw();
});

// A reload starts with no chat open, as the window just opened does.
Promise.all([invoke("new_chat").catch(() => {}), showCached()]).then(() => firstRun(state.subscribed, () => {}, S.signedOut)).then(start).then(() => { if (state.subscribed) loadAgents(); });
