import { failed, grow, newTurn } from "../chat/chat.ts";
import { when } from "../chat/chats.ts";
import { $, $$, bar, S } from "../bar/core.ts";
import { hints, refreshHints } from "../bar/hints.ts";
import { notice } from "../chat/media.ts";
import { emptyPage, errLine } from "../lists/memories.ts";
import { chipHtml, progressLine, researchState, researchSummary, STATE_LABEL, type ResearchRow } from "./research-list.ts";
import { esc, plural } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";

// ---------- Research ----------

/// Chats started as researches, which open in their own window.
export const RESEARCHES = new Set<string>();
export async function loadResearches() {
  const ids = await invoke<string[]>("research_sessions").catch((): string[] => []);
  for (const id of ids) RESEARCHES.add(id);
}

/// Starts a research on `topic` in its own window.
export async function research(topic: string) {
  topic = topic.trim();
  if (!topic) return;
  try {
    const id = await invoke<string>("research_begin", { topic });
    RESEARCHES.add(id);
    const ask = $<HTMLTextAreaElement>("#ask");
    if (ask && ask.value.trim() === topic) { ask.value = ""; grow(ask); refreshHints(); }
  } catch (err) {
    if (S.view === "research") { RL.error = String(err); return drawResearches(); }
    failed(newTurn(""), String(err));
  }
}

/// The bar's Research view: every research on the account, newest first, from
/// the server, so ones started on another computer show too.
export const RL: { list: ResearchRow[]; loaded: boolean; sel: number; mode: "list" | "actions"; error: string } = { list: [], loaded: false, sel: 0, mode: "list", error: "" };

export function renderResearches(topic = "") {
  S.view = "research";
  invoke("close_group").catch(() => {});
  Object.assign(RL, { mode: "list", error: "", sel: 0 });
  drawResearches();
  const field = $<HTMLInputElement>("#rtopic");
  if (field) { field.value = topic; field.focus(); }
  loadResearchList();
}

export async function loadResearchList() {
  try {
    RL.list = await invoke<ResearchRow[]>("list_researches");
    for (const r of RL.list) RESEARCHES.add(r.session);
    RL.loaded = true;
    RL.error = "";
  } catch (e) {
    RL.error = String(e);
  }
  if (S.view === "research") drawResearches();
}

export function researchLine(r: ResearchRow) {
  const parts = [progressLine(r)];
  if (r.sources) parts.push(plural(r.sources, "source", "sources"));
  parts.push(when(r.finished_at ?? r.started_at));
  return parts.join(" · ");
}

export function drawResearches() {
  const typed = $<HTMLInputElement>("#rtopic")?.value ?? "";
  const focused = document.activeElement?.id === "rtopic";
  RL.sel = Math.max(0, Math.min(RL.sel, RL.list.length - 1));
  const sum = RL.loaded ? researchSummary(RL.list) : "";
  const head = `<div class="dhead"><span class="tt"><b>Research</b><small>${esc(sum)}</small></span></div>`;
  const field = `<div class="dinput"><input id="rtopic" placeholder="What should the team research?" autocomplete="off" spellcheck="false"><kbd>⌘R</kbd></div>`;
  const empty = RL.loaded && !RL.list.length;
  let list, keys;
  if (empty) {
    // A paper's icon, as a research shows in the window's sidebar.
    list = emptyPage(ICON.doc, "No research yet.", "A team of agents digs into a question and writes it up as a paper.");
    keys = [[ICON.ret, "research"], ["esc", "back"]];
  } else {
    const rows = RL.list.map((r, i) => {
      const cur = i === RL.sel && !typed.trim();
      const state = researchState(r.status);
      // Export replaces the chip while the row's actions show.
      const tail = cur && RL.mode === "actions" && r.sources
        ? '<span class="acts"><button class="dbtn" data-rl="pdf">Export PDF</button></span>' : chipHtml(r.status, STATE_LABEL[state]);
      const off = state === "failed" || state === "stopped";
      return `<div class="drt${off ? " off" : ""}" role="option" aria-selected="${cur}" data-i="${i}"><span class="ic">${ICON.doc}</span>
        <span class="tt"><b>${esc(r.topic)}</b><small>${esc(researchLine(r))}</small></span>${tail}</div>`;
    }).join("");
    list = `<div class="dlist top" role="listbox" aria-label="Research">${rows}</div>`;
    keys = typed.trim() ? [[ICON.ret, "research"], ["esc", "back"]]
      : RL.mode === "actions" ? [["tab", "hide actions"], ["esc", "back"]]
      : [["↑↓", "move"], [ICON.ret, "open"], ["tab", "actions"], ["esc", "back"]];
  }
  bar.innerHTML = field + head + errLine(RL.error) + list + hints(keys);
  if (!empty) bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  const input = $$<HTMLInputElement>("#rtopic");
  input.value = typed;
  if (focused || !RL.list.length) input.focus();
  input.oninput = () => { const at = input.selectionStart; drawResearches(); $$<HTMLInputElement>("#rtopic").setSelectionRange(at, at); };
  bar.onclick = researchClick;
}

export function researchClick(e: MouseEvent) {
  if (S.view !== "research") return;
  const t = e.target as HTMLElement;
  const btn = t.closest<HTMLElement>("[data-rl]");
  const row = t.closest<HTMLElement>(".drt[data-i]");
  if (row) RL.sel = Number(row.dataset.i);
  if (btn) return researchListAction(btn.dataset.rl!);
  if (row) return researchListAction("open");
}

export async function researchListAction(a: string) {
  const r = RL.list[RL.sel];
  if (!r) return;
  try {
    if (a === "open") return await invoke("open_research", { session: r.session });
    if (a === "pdf") {
      const to = await invoke<string>("export_research", { session: r.session });
      RL.error = "";
      RL.mode = "list";
      drawResearches();
      return notice(`Saved ${to.split("/").pop()} to Downloads`, { path: to });
    }
  } catch (e) {
    RL.error = String(e);
    drawResearches();
  }
}

/// Cmd-R: researches what is typed, and otherwise opens the Research view.
export function researchShortcut() {
  const typed = (S.view === "chat" ? $<HTMLTextAreaElement>("#ask") : S.view === "research" ? $<HTMLInputElement>("#rtopic") : null)?.value.trim();
  if (typed) return research(typed);
  if (S.view !== "research") return renderResearches();
}

export function researchKeys(e: KeyboardEvent) {
  const typed = $<HTMLInputElement>("#rtopic")?.value.trim();
  if (e.key === "Enter" && !e.isComposing) {
    e.preventDefault();
    return typed ? research(typed) : researchListAction("open");
  }
  if (typed) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    RL.sel = Math.max(0, Math.min(RL.list.length - 1, RL.sel + (e.key === "ArrowDown" ? 1 : -1)));
    RL.mode = "list";
    drawResearches();
  } else if (e.key === "Tab") {
    e.preventDefault();
    RL.mode = RL.mode === "actions" ? "list" : "actions";
    drawResearches();
  }
}

