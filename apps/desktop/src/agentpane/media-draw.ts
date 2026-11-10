// Draws the pane's docks: their tab strips, controls and the view of the tab
// each shows, and handles clicks and the menu on its tabs.
import { createElement, Fragment } from "react";
import { createPortal } from "react-dom";
import { removeSessionPortal, setSessionChild, setSessionPortal } from "../agents/session-frame.tsx";
import { SubagentsView, type SubagentRow } from "./subagents-view.tsx";
import { PaneTabs, PhonePaneTabs, type PaneTabItem } from "./tabs-view.tsx";
import { viewKey, rememberScroll } from "./view-memory.ts";
import { isHtmlFile } from "./html-preview-state.ts";
import { baseName, type Dock, DOCKS, moveTab } from "./media-state.ts";
import type { Artifact } from "./artifacts-state.ts";
import { artifactCount, artifactsView, checkArtifacts, setArtifacts, showFrames } from "./artifacts.ts";
import { changesView, madeChangesView, revalidateChanges } from "./changes.ts";
import { closeFilesMenu, filesView, revalidateFiles } from "./files.ts";
import { drawWorktrees } from "./worktree.ts";
import { drag } from "./drag.ts";
import { readError } from "./read.ts";
import { viewOf } from "./views.ts";
import { h } from "../ui/dom.ts";
import { Icon, type IconName } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { runningCount, runningView } from "./running.ts";
import { onShownChange } from "../ui/active.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { writeClipboard } from "../ui/parts.ts";
import { type TabKind, ARTS, CHANGES, closeTab, FILES, newTerminal, P, previewTab, RUNNING, saveLayout, saveWorkFile, showTab, SUBS, type MediaTab, focusTab } from "./media.ts";
import { docked, main, paneOf, place, shell } from "./media-layout.ts";

// ----- Tabs -----

/// What the log has now for the Artifacts tab: its artifacts, and when
/// each turn started, by its message's place.
interface PaneArtifacts {
  list: Artifact[];
  turnStart: (turn: number) => number | undefined;
}

/// Draws the pane again: its tabs and the one on screen.
export function drawPane(subs?: { rows: SubagentRow[]; count: number }, arts?: PaneArtifacts) {
  if (subs) { P.subs = subs.rows; P.subCount = subs.count; }
  if (arts) setArtifacts(arts.list, arts.turnStart);
  draw();
}

export function draw() {
  // A tab being dragged is not drawn over until it drops.
  if (drag?.ghost) return;
  const split = docked();
  for (const d of DOCKS) drawDock(d, split[d]);

  for (const d of DOCKS) {
    const host = paneOf(d)?.querySelector<HTMLElement>(".agwtw");
    if (host) drawWorktrees(host, P.on[d] === FILES || P.on[d] === CHANGES);
  }

  place();
  phoneTabs();
}

/// On a phone the session shows one pane at a time: Log, the conversation, or one of the
/// pane's tabs filling the screen. The tabs are the bottom dock's, whose own strip is
/// hidden by mobile.css; `mb-log` on the shell says the conversation is the one shown.
function phoneTabs() {
  const el = shell(), box = main();
  if (!el || !box || !document.body.classList.contains("mobile")) return;
  let nav = box.querySelector<HTMLElement>(":scope > #agmnav");

  if (!nav) {
    nav = document.createElement("nav");
    nav.id = "agmnav";
    nav.className = "agmnav";
    box.prepend(nav);
    el.classList.add("mb-log");
  }

  const on = el.classList.contains("mb-log") ? "log" : P.on.bottom;
  const tabs = [...(paneOf("bottom")?.querySelectorAll<HTMLElement>(".agtabs > [data-mtab]") ?? [])]
    .map(t => ({ id: t.dataset.mtab!, name: t.querySelector("span")?.textContent ?? "" }));
  setSessionPortal(nav, createElement(PhonePaneTabs, { tabs: [{ id: "log", name: "Log" }, ...tabs], selected: on, select: (id: string) => {
    el.classList.toggle("mb-log", id === "log");
    if (id !== "log") P.on.bottom = id;
    draw();
  } }));
}

/// Draws dock `d` with tabs `ids`, moving into it the views of tabs that
/// came from the other.
function drawDock(d: Dock, ids: string[]) {
  const pane = paneOf(d);
  const tabs = pane?.querySelector<HTMLElement>(".agtabs"), views = pane?.querySelector<HTMLElement>(".agpviews");
  if (!pane || !tabs || !views) return;
  // The tab on screen went: the newest one here.
  if (!ids.includes(P.on[d])) P.on[d] = ids.at(-1) ?? FILES;
  const on = P.on[d];
  const items: PaneTabItem[] = ids.map(id => {
    if (id === FILES) return { id, name: "Files", icon: TAB_ICONS.files, count: null, closeIcon: null };
    if (id === CHANGES) return { id, name: "Changes", icon: TAB_ICONS.changes, count: null, closeIcon: null };
    if (id === SUBS) return { id, name: "Subagents", icon: TAB_ICONS.subagents, count: P.subCount, closeIcon: null };
    if (id === RUNNING) return { id, name: "Running", icon: TAB_ICONS.running, count: runningCount(), closeIcon: null };
    if (id === ARTS) return { id, name: "Artifacts", icon: TAB_ICONS.artifacts, count: artifactCount(), closeIcon: null };
    const item = P.tabs.find(tab => tab.id === id)!;
    return { id, name: item.name, icon: TAB_ICONS[item.kind], count: null, closeIcon: "x" };
  });
  setSessionPortal(tabs, createElement(PaneTabs, { tabs: items, selected: on }));
  const bottom = d === "bottom";
  const dock = pane.querySelector<HTMLElement>('[data-pact="dock"]')!;
  dock.title = bottom ? "Dock these tabs on the right" : "Dock these tabs at the bottom";
  const fold = pane.querySelector<HTMLElement>('[data-pact="fold"]')!;
  const strip = document.getElementById(`agpstrip-${d}`)!;
  const shown = P.tabs.find(t => t.id === on);
  const fixed = on === RUNNING ? RUNNING : on === SUBS ? SUBS : on === ARTS ? ARTS : on === CHANGES ? CHANGES : FILES;
  const label = shown ? shown.name : { [RUNNING]: "Running", [SUBS]: "Subagents", [ARTS]: "Artifacts", [CHANGES]: "Changes", [FILES]: "Files" }[fixed];
  const icon = (html: string) => createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: html } });
  setSessionChild(`dock-controls:${d}`, createElement(Fragment, null,
    createPortal(icon(bottom ? DOCK_RIGHT : DOCK_BOTTOM), dock, "dock"),
    createPortal(icon(bottom ? FOLD_DOWN : FOLD_RIGHT), fold, "fold"),
    createPortal(createElement(Fragment, null, createElement("span", { style: { display: "contents" } }, createElement(Icon, { name: TAB_ICONS[shown?.kind ?? fixed] })), createElement("span", null, label), createElement("small", null, ids.length)), strip, "strip")));
  // The Files view is made once per session, wherever it sits.
  const made = filesView();
  let filesEl = document.querySelector<HTMLElement>(".agpviews > .agpview.files");
  if (filesEl !== made) { filesEl?.remove(); filesEl = made; }

  if (ids.includes(FILES)) {
    if (filesEl.parentElement !== views) views.prepend(filesEl);
    filesEl.hidden = on !== FILES;
    if (on !== FILES) closeFilesMenu();
    revalidateFiles(on === FILES && !P.layout.hidden[d]);
  }

  // The Changes view is made the first time it shows, as it runs git; shown
  // again, it reads only when the folder changed.
  const changesEl = on === CHANGES ? changesView() : madeChangesView();

  if (changesEl && ids.includes(CHANGES)) {
    if (changesEl.parentElement !== views) views.append(changesEl);
    changesEl.hidden = on !== CHANGES;
    revalidateChanges(on === CHANGES && !P.layout.hidden[d]);
  }

  // So is the Artifacts view, while there are any.
  const artsEl = artifactsView();

  if (!artifactCount()) artsEl.remove();
  else if (ids.includes(ARTS)) {
    if (artsEl.parentElement !== views) views.append(artsEl);
    artsEl.hidden = on !== ARTS;
    showFrames(on === ARTS && !P.layout.hidden[d]);
    if (on === ARTS) checkArtifacts();
  }

  const runningEl = runningView();

  if (runningEl && ids.includes(RUNNING)) {
    if (runningEl.parentElement !== views) views.append(runningEl);
    runningEl.hidden = on !== RUNNING;
  }

  // Subagents redraw each time; a file's view is drawn once and kept.
  let subView = document.querySelector<HTMLElement>(".agpviews > .agpview.subs");

  if (!P.subCount && subView) { removeSessionPortal(subView); subView.remove(); }
  else if (ids.includes(SUBS)) {
    if (!subView) subView = h('<div class="agpview subs"></div>');
    if (subView.parentElement !== views) views.append(subView);
    setSessionPortal(subView, createElement(SubagentsView, { rows: P.subs }));
    subView.hidden = on !== SUBS;
  }

  for (const t of P.tabs) {
    if (!ids.includes(t.id)) continue;
    if (!t.el) {
      t.el = viewOf(t);
      if (P.session && t.text) rememberScroll(t.el, `${viewKey(P.session.machine_id, P.session.id)}:file:${t.id}`);
    }
    if (t.el.parentElement !== views) views.append(t.el);
    const hide = t.id !== on || P.layout.hidden[d];
    if (hide && !t.el.hidden) stopPlaying(t);
    if (!hide && t.el.hidden) showFrame(t);
    t.el.hidden = t.id !== on;
    if (!hide) t.model?.resize();
    if (!hide && t.term) requestAnimationFrame(() => {
      t.term?.screen.fit();
      // A refresh must not take the keys back from the conversation.
      // Check now, since focus may have changed while this frame waited.
      if (t.id === P.on[d] && !P.layout.hidden[d] && pane.isConnected && (document.activeElement === pane || t.term?.screen.focused())) t.term?.screen.focus();
    });
  }
}

type Fixed = "subagents" | "files" | "changes" | "artifacts" | "running";

const TAB_ICONS: Record<TabKind | Fixed, IconName> = {
  files: "paneFiles", changes: "paneChanges", text: "paneText", term: "paneTerm", image: "paneImage", model: "paneModel",
  video: "paneVideo", pdf: "panePdf", artifacts: "paneArtifacts", running: "paneRunning", subagents: "paneSubagents",
};
export const DOCK_RIGHT = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/></svg>';
export const DOCK_BOTTOM = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 14h18"/></svg>';
const FOLD_RIGHT = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>';
const FOLD_DOWN = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 9l6 6 6-6"/></svg>';

/// Pauses a tab's video and its model's animation, whose frames would
/// otherwise keep drawing out of sight.
function stopPlaying(t: MediaTab) {
  t.el?.querySelector("video")?.pause();
  t.model?.anim?.play(false);
}

/// Loads a tab's video again where it stood: WebKit can lose its picture
/// while it is out of sight and show it dark, sound and controls still there.
function showFrame(t: MediaTab) {
  const video = t.el?.querySelector("video");
  if (!video || video.readyState === 0) return;
  const at = video.currentTime;
  video.addEventListener("loadedmetadata", () => { video.currentTime = at; }, { once: true });
  video.load();
}

// The window hid or the pane went: nothing plays on. Shown again, the
// video in view gets its picture back.
onShownChange(shown => {
  for (const t of P.tabs) {
    if (!shown) stopPlaying(t);
    else if (t.el && !t.el.hidden) showFrame(t);
  }
}, true);

/// The menu of a tab the user opened: close it, save its file, copy its path. The
/// fixed tabs (Files, Changes, Subagents, Artifacts) have none.
export function tabMenu(e: MouseEvent) {
  const id = (e.target as HTMLElement).closest<HTMLElement>("[data-mtab]")?.dataset.mtab;
  const t = P.tabs.find(x => x.id === id);
  if (!t) return;
  const items: MenuItem[] = [];
  const note = (text: string) => { const m = t.el?.querySelector(".agpacts .meta"); if (m) m.textContent = text; };
  const path = t.kind === "text" ? t.rel : t.path;
  if (t.kind === "text" && t.rel && isHtmlFile(t.rel) && !t.text?.binary) items.push({ label: t.preview?.active ? "Source" : "Preview", on: () => { focusTab(t.id); if (t.preview?.active) t.el?.querySelector<HTMLButtonElement>("[data-preview]")?.click(); else previewTab(t); } });
  if (path) items.push({ label: "Copy path", on: () => void writeClipboard(path) });

  if (t.kind === "text" && t.rel) {
    const rel = t.rel;
    items.push({ label: "Save", on: () => saveWorkFile(rel, note, t.root) });
  } else if (t.path && P.session) {
    const { machine_id } = P.session, file = t.path;
    items.push({ label: "Save", on: () => void invoke<string>("agent_file_save", { machineId: machine_id, path: file })
      .then(to => { note(`Saved to Downloads as ${baseName(to)}`); return invoke("reveal_file", { path: to }); })
      .catch(err => note(readError(err))) });
  }

  items.push({ label: "Close", on: () => closeTab(t.id) });
  openMenu(e, t.name, items);
}

export function paneClick(e: MouseEvent, d: Dock) {
  const target = e.target as HTMLElement;
  const close = target.closest<HTMLElement>("[data-mclose]");
  if (close) { e.stopPropagation(); return closeTab(close.dataset.mclose!); }
  const tab = target.closest<HTMLElement>("[data-mtab]");

  if (tab) {
    if (P.dragged) return;
    return showTab(d, tab.dataset.mtab!);
  }

  const act = target.closest<HTMLElement>("[data-pact]")?.dataset.pact;

  if (act === "dock") {
    // This dock's tabs go to the other, which shows the one on screen here.
    const to: Dock = d === "right" ? "bottom" : "right", on = P.on[d];
    for (const id of docked()[d]) P.layout = moveTab(P.layout, id, to);
    for (const id of P.moved.keys()) if (docked()[d].includes(id)) P.moved.set(id, to);
    P.layout.dock = to;
    P.on[to] = on;
    saveLayout();
    return draw();
  }

  if (act === "fold") { P.layout.hidden[d] = true; saveLayout(); return draw(); }
  if (act === "term") return newTerminal(d);
}
