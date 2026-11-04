// The command center's Research page and its sidebar group.
import { since } from "../ui/format.ts";
import { dbtn as button, emptyPanel, errorNote, svgEl as icon } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { el as h } from "../project/projects-state.ts";
import { newestFirst, phaseStep, PHASES, progressLine, researchState, researchSummary, STATE_LABEL, type ResearchRow } from "./research-list.ts";
import { RESEARCHES } from "./researches.ts";
import { IC, views } from "../center/state.ts";
import { failure, tag, skeleton } from "../center/pieces.ts";
import { shown, registerView, go, drawTop } from "../center/views.ts";
import { registerSidebarGroup, reloadGroup, sideItem, reloadChats, plusButton, emptyRow } from "../center/sidebar.ts";

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
  if (shown === "research") { drawResearch(); drawTop(); }
  return research.list;
}

/// Opens a research in its own window, as the bar's list does; its chat
/// when the window cannot open.
export function openResearch(session: string) {
  invoke("open_research", { session }).catch(e => {
    console.warn("opening a research:", e);
    go("pane", { chat: { session } });
  });
}

/// Starts a research on what the page's field holds, in its own window.
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

/// The page's field: what to research, and the button that starts it.
function researchField() {
  const input = h("input");
  input.type = "text";
  input.placeholder = "What should the team research?";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", "What should the team research?");
  input.value = research.draft;
  input.disabled = research.starting;
  const start = button(research.starting ? "Starting…" : "Research", "pri small", () => startResearch());
  start.disabled = research.starting;
  input.addEventListener("input", () => { research.draft = input.value; });
  input.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.isComposing) { e.preventDefault(); startResearch(); }
  });
  return h("div", "cc-rnew", icon(IC.doc), input, start);
}

/// A research's status chip.
function researchTag(r: ResearchRow) {
  const st = researchState(r.status);
  const cls = st === "running" ? "run" : st === "waiting" ? "wait" : st === "failed" ? "bad" : "";
  return tag(cls, st === "running" ? h("span", "dspin") : null, STATE_LABEL[st]);
}

/// The Progress cell: the running round's steps as a bar, then its line.
function researchProgress(r: ResearchRow) {
  const cell = h("span", "");
  const step = phaseStep(r.status);
  if (step) cell.append(h("span", "cc-rsteps", ...PHASES.map((_, i) => h("i", i + 1 < step ? "done" : i + 1 === step ? "now" : ""))));
  cell.append(h("span", "num", progressLine(r)));
  return cell;
}

/// One research per row: its question, status, progress, sources and start.
function researchTable(list: ResearchRow[]) {
  const table = h("div", "cc-mt cc-rt", ...["Research", "Status", "Progress", "Sources", "Started"].map(t => h("span", "th", t)));
  table.setAttribute("role", "table");
  for (const r of list) {
    const off = ["failed", "stopped"].includes(researchState(r.status)) ? " off" : "";
    const open = h("button", "", h("span", "", r.topic));
    open.type = "button";
    open.title = r.topic;
    const cells = [
      h("span", `nm${off}`, h("b", "", open)),
      h("span", off.trim(), researchTag(r)),
      researchProgress(r),
      h("span", `num${off}`, r.sources ? String(r.sources) : "–"),
      h("span", `num${off}`, since(r.started_at)),
    ];
    if (off) cells[2].classList.add("off");
    cells[4].title = new Date(r.started_at).toLocaleString();
    for (const c of cells) {
      c.classList.add("r");
      c.addEventListener("click", () => openResearch(r.session));
    }
    table.append(...cells);
  }
  return table;
}

/// The Research page into its view, keeping its scroll and the field's
/// focus; `focus` puts the caret in the field.
function drawResearch(focus = false) {
  const el = views.get("research")?.el;
  if (!el) return;
  const old = el.querySelector<HTMLElement>(":scope > .cc-view");
  const typing = !!old && old.querySelector(".cc-rnew input") === document.activeElement;
  const field = researchField();
  const kids: Node[] = [field];
  if (research.error) kids.push(errorNote(research.error));
  const list = research.list;
  if (list == null) {
    if (!research.error) kids.push(h("div", "cc-panel", ...skeleton([70, 55, 62])));
  } else if (!list.length) {
    kids.push(emptyPanel(h("b", "", "No research yet"), h("span", "", "Type a question above to start one.")));
  } else {
    kids.push(h("div", "", h("div", "cc-sec", h("h4", "", "Research")), researchTable(list)));
  }
  const view = h("div", "cc-view", ...kids);
  if (old) { const top = old.scrollTop; old.replaceWith(view); view.scrollTop = top; } else el.append(view);
  if (focus || typing) field.querySelector("input")!.focus();
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
  load: async () => (await fetchResearch())?.slice(0, SIDE_RESEARCH) ?? null,
  row(r) {
    const b = sideItem(IC.doc, r.topic, 0);
    b.title = `${r.topic}\n${STATE_LABEL[researchState(r.status)]}`;
    if (researchState(r.status) === "running") b.append(h("span", "dspin cc-run"));
    return b;
  },
  onPick: r => openResearch(r.session),
});
