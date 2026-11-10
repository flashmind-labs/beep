import { createElement } from "react";
import { flushSync } from "react-dom";
import type { Root } from "react-dom/client";
import { openBarPage } from "../bar/page-root.ts";
import { ResearchesView } from "./researches-view.tsx";
import { failed, grow, newTurn } from "../chat/chat.ts";
import { $, S } from "../bar/core.ts";
import { hintModel, refreshHints } from "../bar/hints.ts";
import { notice } from "../chat/media.ts";
import { progressLine, type ResearchRow } from "./research-list.ts";
import { formatAgo, plural } from "../ui/format.ts";
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
    if (S.view === "chat") failed(newTurn(""), String(err));
    else notice(`Could not start research: ${String(err)}`, { bad: true });
  }
}

/// The bar's Research view: every research on the account, newest first, from
/// the server, so ones started on another computer show too.
export const RL: { list: ResearchRow[]; loaded: boolean; sel: number; mode: "list" | "actions"; error: string } = { list: [], loaded: false, sel: 0, mode: "list", error: "" };

let view: { host: HTMLElement; root: Root } | null = null;
let generation = 0, readVersion = 0, initial = "";
function disposeResearches() { view = null; }
export function renderResearches(topic = "") {
  generation++; initial = topic;
  S.view = "research";
  invoke("close_group").catch(() => {});
  Object.assign(RL, { mode: "list", error: "", sel: 0 });
  view = openBarPage(disposeResearches);
  drawResearches();
  loadResearchList();
}

export async function loadResearchList() {
  const mine = generation, read = ++readVersion;
  try {
    const list = await invoke<ResearchRow[]>("list_researches");
    if (mine !== generation || read !== readVersion) return;
    RL.list = list;
    for (const r of RL.list) RESEARCHES.add(r.session);
    RL.loaded = true;
    RL.error = "";
  } catch (e) {
    if (mine !== generation || read !== readVersion) return;
    RL.error = String(e);
  }

  if (S.view === "research") drawResearches();
}

export function researchLine(r: ResearchRow) {
  const parts = [progressLine(r)];
  if (r.sources) parts.push(plural(r.sources, "source", "sources"));
  parts.push(formatAgo(r.finished_at ?? r.started_at, "chat"));
  return parts.join(" · ");
}

export function drawResearches() {
  const current = view;
  if (!current?.host.isConnected || S.view !== "research") return;
  const typed = ($<HTMLInputElement>("#rtopic")?.value ?? initial).trim();
  RL.sel = Math.max(0, Math.min(RL.sel, RL.list.length - 1));
  const empty = RL.loaded && !RL.list.length;
  const keys = empty || typed ? [[ICON.ret, "research"], ["esc", "back"]]
    : RL.mode === "actions" ? [["tab", "hide actions"], ["esc", "back"]]
    : [["↑↓", "move"], [ICON.ret, "open"], ["tab", "actions"], ["esc", "back"]];
  flushSync(() => current.root.render(createElement(ResearchesView, {
    list: RL.list, loaded: RL.loaded, selected: RL.sel, mode: RL.mode, error: RL.error, initial, typed: !!typed, hints: hintModel(keys),
    edit: drawResearches, choose: (index: number, action: "open" | "pdf") => { RL.sel = index; void researchListAction(action); },
  })));
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

