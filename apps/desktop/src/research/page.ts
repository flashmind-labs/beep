// The command center's Research page and its sidebar group.
import { invoke } from "../ui/tauri.ts";
import { newestFirst, researchState, researchSummary, STATE_LABEL, type ResearchRow } from "./research-list.ts";
import { RESEARCHES } from "./researches.ts";
import { state, views } from "../center/state.ts";
import { groupShown } from "../settings/subscription.ts";
import { failure } from "../ui/pieces.ts";
import { createElement } from "react";
import { showWindowParts } from "../center/window-root.ts";
import { ResearchPage } from "./page-view.tsx";
import { currentLocation, registerView, go, drawTop, openResearch } from "../center/views.ts";
import { registerSidebarGroup, reloadGroup, reloadChats } from "../center/sidebar.ts";
import { IC } from "../ui/icons.tsx";

/// Every research on the account, newest first (null before the first
/// answer), why the last read or start failed, whether one is starting and
/// what the page's field holds.
export const research = { list: null as ResearchRow[] | null, error: "", starting: false, draft: "" };
/// The researches the sidebar lists under its Research heading.
const SIDE_RESEARCH = 3;

/// Reads the account's researches. Their chats leave the Chats list, so a
/// research started on another device reloads it.
async function fetchResearch() {
  try {
    const list = await invoke<ResearchRow[]>("list_researches");
    const added = list.some(r => !RESEARCHES.has(r.session));
    for (const r of list) RESEARCHES.add(r.session);
    research.list = newestFirst(list);
    research.error = "";
    if (added) reloadChats();
  } catch (e) {
    console.warn("listing researches:", e);
    research.error = failure(e);
  }

  if (currentLocation()?.view === "research") { drawResearch(); drawTop(); }
  return research.list;
}

/// Starts a research on what the page's field holds; Rust opens it as a page of the window.
async function startResearch() {
  const topic = research.draft.trim();
  if (!topic || research.starting) return;
  research.starting = true;
  drawResearch();

  try {
    const session = await invoke<string>("research_begin", { topic });
    RESEARCHES.add(session);
    research.draft = "";
    research.error = "";
  } catch (e) {
    research.error = failure(e);
  } finally {
    research.starting = false;
  }

  drawResearch(true);
  reloadGroup("research");
}

/// Updates the page without replacing the scroll container or input.
function drawResearch(focus = false) {
  const el = views.get("research")?.el;
  if (!el) return;
  showWindowParts([{ part: "research-list", host: el, content: createElement(ResearchPage, {
    list: research.list,
    error: research.error,
    starting: research.starting,
    draft: research.draft,
    onDraft: (value: string) => { research.draft = value; },
    onStart: () => void startResearch(),
    onOpen: openResearch,
  }) }]);
  if (focus) el.querySelector<HTMLInputElement>(".cc-rnew input")?.focus();
}

registerView("research", {
  title: () => ({ title: "Research", sub: research.list ? researchSummary(research.list) : "" }),
  mount() {
    drawResearch(true);
    reloadGroup("research");
  },
});

// Under Projects: the newest researches, a spinner on those running. The
// title opens the Research page, + its field.
registerSidebarGroup<ResearchRow>("research", {
  order: 5.2,
  title: "Research",
  onTitle: () => go("research"),
  action: { label: "New research", run: () => { go("research"); } },
  empty: "No research yet",
  load: async () => (groupShown("research", state.subscribed) ? (await fetchResearch())?.slice(0, SIDE_RESEARCH) ?? null : null),
  key: r => r.session,
  render: r => {
    const location = currentLocation(), current = location?.view === "researchopen" && location.arg.session === r.session;
    return createElement("button", { type: "button", className: "cc-si", title: `${r.topic}\n${STATE_LABEL[researchState(r.status)]}`, "aria-label": r.topic, "aria-current": current ? "true" : undefined, onClick: () => openResearch(r.session) },
      createElement("span", { className: "sidebar-icon", style: { display: "contents" }, dangerouslySetInnerHTML: { __html: IC.doc } }), createElement("span", { className: "tt" }, r.topic), researchState(r.status) === "running" ? createElement("span", { className: "dspin cc-run" }) : null);
  },

});
