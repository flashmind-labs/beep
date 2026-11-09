// The command center's Research page and its sidebar group.
import { invoke } from "../ui/tauri.ts";
import { el as h } from "../ui/dom.ts";
import { newestFirst, researchState, researchSummary, STATE_LABEL, type ResearchRow } from "./research-list.ts";
import { RESEARCHES } from "./researches.ts";
import { state, views } from "../center/state.ts";
import { groupShown } from "../settings/subscription.ts";
import { failure } from "../ui/pieces.ts";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { ResearchPage } from "./page-view.tsx";
import { currentLocation, registerView, go, drawTop, openResearch } from "../center/views.ts";
import { registerSidebarGroup, reloadGroup, sideItem, reloadChats, plusButton, emptyRow } from "../center/sidebar.ts";
import { IC } from "../ui/icons.ts";

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

let root: Root | null = null;

/// Updates the page without replacing the scroll container or input.
function drawResearch(focus = false) {
  const el = views.get("research")?.el;
  if (!el) return;
  root ??= createRoot(el);
  flushSync(() => root?.render(createElement(ResearchPage, {
    list: research.list,
    error: research.error,
    starting: research.starting,
    draft: research.draft,
    onDraft: (value: string) => { research.draft = value; },
    onStart: () => void startResearch(),
    onOpen: openResearch,
  })));
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
  action: () => plusButton("New research", () => go("research")),
  empty: () => emptyRow("No research yet"),
  load: async () => (groupShown("research", state.subscribed) ? (await fetchResearch())?.slice(0, SIDE_RESEARCH) ?? null : null),
  row(r) {
    const b = sideItem(IC.doc, r.topic, 0);
    b.title = `${r.topic}\n${STATE_LABEL[researchState(r.status)]}`;
    if (researchState(r.status) === "running") b.append(h("span", "dspin cc-run"));
    const location = currentLocation();
    if (location?.view === "researchopen" && location.arg.session === r.session) b.setAttribute("aria-current", "true");
    return b;
  },
  onPick: r => openResearch(r.session),
});
