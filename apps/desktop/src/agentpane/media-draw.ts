// Draws the pane's docks: works out what each shows (its tab strip,
// controls and the view of each tab) for dock-view.tsx, and handles the
// clicks and the menu on its tabs.
import type { SubagentRow } from "./subagents-view.tsx";
import type { PaneTabItem } from "./tabs-view.tsx";
import { isHtmlFile } from "./html-preview-state.ts";
import { baseName, type Dock, DOCKS, moveTab } from "./media-state.ts";
import type { Artifact } from "./artifacts-state.ts";
import { artifactCount, artifactsTab, setArtifacts } from "./artifacts.ts";
import { changesTab } from "./changes.ts";
import { filesTab } from "./files.ts";
import { worktreeMenu } from "./worktree.ts";
import { drag } from "./drag.ts";
import { readError } from "./read.ts";
import { togglePreview } from "./text-view.tsx";
import type { IconName } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { runningCount } from "./running.ts";
import { onShownChange } from "../ui/active.ts";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { writeClipboard } from "../ui/parts.ts";
import { type TabKind, ARTS, CHANGES, closeTab, FILES, P, previewTab, RUNNING, saveLayout, saveWorkFile, showTab, SUBS, type MediaTab, focusTab } from "./media.ts";
import { type DockModel, type DockView, docked, paneModel, paneOf, place, setPaneModel, shell } from "./media-layout.ts";

// ----- Tabs -----

/// What the log has now for the Artifacts tab: its artifacts, and when
/// each turn started, by its message's place.
interface PaneArtifacts {
  list: Artifact[];
  turnStart: (turn: number) => number | undefined;
}

/// What the log last gave the pane, as JSON, and for which session.
let given: { session: typeof P.session; key: string } | null = null;

/// Draws the pane again: its tabs and the one on screen. A log draw that
/// gives it the same subagents and artifacts, as each streamed word does,
/// leaves it as it is; the pane draws itself when its own state changes.
export function drawPane(subs?: { rows: SubagentRow[]; count: number }, arts?: PaneArtifacts) {
  const key = JSON.stringify([subs ?? null, arts?.list ?? null]);
  if (given?.session === P.session && given.key === key) return;
  given = { session: P.session, key };
  if (subs) { P.subs = subs.rows; P.subCount = subs.count; }
  if (arts) setArtifacts(arts.list, arts.turnStart);
  draw();
}

/// Brings each dock's views up to date with what they show, then draws.
export function draw() {
  // A tab being dragged is not drawn over until it drops.
  if (drag?.ghost) return;
  const split = docked();
  for (const d of DOCKS) syncDock(d, split[d]);
  publish(split);
}

/// Shows the tab now in `P.on[d]` when nothing else changed: dock `d`'s views flip and
/// it draws again alone, without laying the docks out. False when the docks need `draw`.
export function flipTab(d: Dock) {
  const { docks, phone } = paneModel.getState();
  if (drag?.ghost || phone || !docks?.[d].shown || DOCKS.some(x => docks[x].zoomed !== (P.zoom === x))) return false;
  const ids = docked()[d];
  if (ids.length !== docks[d].tabs.length || ids.some((id, i) => docks[d].tabs[i].id !== id)) return false;
  syncDock(d, ids);
  setPaneModel({ docks: { ...docks, [d]: dockModel(d, ids) } });
  refreshTopicSubscriptions();
  return true;
}

/// Draws tab `t` again after its own state changed: its load, terminal or note.
export function touchTab(t: MediaTab) {
  t.rev++;
  if (P.tabs.includes(t) && !drag?.ghost) publish(docked());
}

/// Hands the docks' model to dock-view.tsx and lays them out.
function publish(split: Record<Dock, string[]>) {
  // The center window's phone layout, which center/mobile.ts marks on the body.
  const phone = shell() && document.body.classList.contains("mobile") ? phoneTabs(split.bottom) : null;
  // On a phone the session first shows the conversation.
  if (phone && !P.phoneShown) { P.phoneShown = true; setPaneModel({ log: true }); }
  const log = paneModel.getState().log;
  place({ right: dockModel("right", split.right), bottom: dockModel("bottom", split.bottom) }, { phone: phone && { tabs: phone, selected: log ? "log" : P.on.bottom } });
}

/// On a phone the session shows one pane at a time: Log, the conversation, or one of the
/// bottom dock's tabs filling the screen; mobile.css hides the dock's own strip.
function phoneTabs(ids: string[]) {
  return [{ id: "log", name: "Log" }, ...ids.map(id => ({ id, name: paneTabItem(id).name }))];
}

/// A tab picked in the phone's strip.
export function pickPhoneTab(id: string) {
  setPaneModel({ log: id === "log" });
  if (id !== "log") P.on.bottom = id;
  draw();
}

/// The tab strip item of tab `id`.
export function paneTabItem(id: string): PaneTabItem {
  if (id === FILES) return { id, name: "Files", icon: TAB_ICONS.files, count: null, closeIcon: null };
  if (id === CHANGES) return { id, name: "Changes", icon: TAB_ICONS.changes, count: null, closeIcon: null };
  if (id === SUBS) return { id, name: "Subagents", icon: TAB_ICONS.subagents, count: P.subCount, closeIcon: null };
  if (id === RUNNING) return { id, name: "Running", icon: TAB_ICONS.running, count: runningCount(), closeIcon: null };
  if (id === ARTS) return { id, name: "Artifacts", icon: TAB_ICONS.artifacts, count: artifactCount(), closeIcon: null };
  const tab = P.tabs.find(t => t.id === id);
  return { id, name: tab?.name ?? id, icon: TAB_ICONS[tab?.kind ?? "text"], count: null, closeIcon: "x" };
}

/// What dock `d` with tabs `ids` shows.
function dockModel(d: Dock, ids: string[]): DockModel {
  const on = P.on[d];
  const shown = P.tabs.find(t => t.id === on);
  const fixed = on === RUNNING ? RUNNING : on === SUBS ? SUBS : on === ARTS ? ARTS : on === CHANGES ? CHANGES : FILES;
  const has = (id: string) => ids.includes(id);
  const views: DockView[] = [
    ...(has(FILES) ? [{ kind: "files" as const, hidden: on !== FILES }] : []),
    ...(has(CHANGES) && changesMade ? [{ kind: "changes" as const, hidden: on !== CHANGES }] : []),
    ...(has(ARTS) ? [{ kind: "artifacts" as const, hidden: on !== ARTS }] : []),
    ...(has(RUNNING) ? [{ kind: "running" as const, hidden: on !== RUNNING }] : []),
    ...(has(SUBS) ? [{ kind: "subagents" as const, hidden: on !== SUBS }] : []),
    ...P.tabs.filter(t => has(t.id)).map(t => ({ kind: "tab" as const, tab: t, rev: t.rev, hidden: t.id !== on })),
  ];
  const before = paneModel.getState().docks?.[d];
  return {
    tabs: ids.map(paneTabItem), on, views,
    shown: before?.shown ?? false, strip: before?.strip ?? false, size: before?.size ?? null, zoomed: before?.zoomed ?? false,
    label: shown ? shown.name : FIXED_NAMES[fixed], icon: TAB_ICONS[shown?.kind ?? fixed],
    worktrees: on === FILES || on === CHANGES ? worktreeMenu() : null,
  };
}

/// Whether the Changes view of the pane's session was made, as the last draw left it.
let changesMade = false;

/// Brings the views of dock `d`, with tabs `ids`, up to date with what it shows.
function syncDock(d: Dock, ids: string[]) {
  // The tab on screen went: the newest one here.
  if (!ids.includes(P.on[d])) P.on[d] = ids.at(-1) ?? FILES;
  const on = P.on[d], folded = P.layout.hidden[d];
  if (ids.includes(FILES)) filesTab(on === FILES, on === FILES && !folded);
  // The Changes view is made the first time it shows, as it runs git; shown
  // again, it reads only when the folder changed.
  if (ids.includes(CHANGES)) changesMade = changesTab(on === CHANGES, on === CHANGES && !folded);
  if (ids.includes(ARTS)) artifactsTab(on === ARTS, on === ARTS && !folded);

  for (const t of P.tabs) {
    if (!ids.includes(t.id)) continue;
    const hide = t.id !== on || folded;
    if (hide && !t.viewHidden) stopPlaying(t);
    if (!hide && t.viewHidden) showFrame(t);
    t.viewHidden = t.id !== on;
    if (!hide) t.model?.resize();
    if (!hide && t.term) requestAnimationFrame(() => {
      t.term?.screen.fit();
      // A refresh must not take the keys back from the conversation.
      // Check now, since focus may have changed while this frame waited.
      const pane = paneOf(d);
      if (t.id === P.on[d] && !P.layout.hidden[d] && pane && (document.activeElement === pane || t.term?.screen.focused())) t.term?.screen.focus();
    });
  }
}

type Fixed = "subagents" | "files" | "changes" | "artifacts" | "running";

const FIXED_NAMES: Record<Fixed, string> = { running: "Running", subagents: "Subagents", artifacts: "Artifacts", changes: "Changes", files: "Files" };

const TAB_ICONS: Record<TabKind | Fixed, IconName> = {
  files: "paneFiles", changes: "paneChanges", text: "paneText", term: "paneTerm", image: "paneImage", model: "paneModel",
  video: "paneVideo", pdf: "panePdf", artifacts: "paneArtifacts", running: "paneRunning", subagents: "paneSubagents",
};

/// Pauses a tab's video and its model's animation, whose frames would
/// otherwise keep drawing out of sight.
function stopPlaying(t: MediaTab) {
  t.video?.pause();
  t.model?.anim?.play(false);
}

/// Loads a tab's video again where it stood: WebKit can lose its picture
/// while it is out of sight and show it dark, sound and controls still there.
function showFrame(t: MediaTab) {
  const video = t.video;
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
    else if (!t.viewHidden) showFrame(t);
  }
}, true);

/// Says `text` in tab `t`'s footer, as a save or copy did.
export function noteTab(t: MediaTab, text: string) {
  t.note = text;
  touchTab(t);
}

/// The menu of a tab the user opened: close it, save its file, copy its path. The
/// fixed tabs (Files, Changes, Subagents, Artifacts) have none.
export function tabMenu(e: MouseEvent, id: string) {
  const t = P.tabs.find(x => x.id === id);
  if (!t) return;
  e.preventDefault();
  const items: MenuItem[] = [];
  const note = (text: string) => noteTab(t, text);
  const path = t.kind === "text" ? t.rel : t.path;
  if (t.kind === "text" && t.rel && isHtmlFile(t.rel) && !t.text?.binary) items.push({ label: t.preview?.active ? "Source" : "Preview", on: () => { focusTab(t.id); if (t.preview?.active) void togglePreview(t); else previewTab(t); } });
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

/// A click on tab `id` in dock `d`: shows it, unless it ends a drag.
export function pickTab(d: Dock, id: string) {
  if (P.dragged) return;
  showTab(d, id);
}

/// This dock's tabs go to the other, which shows the one on screen here.
export function moveDock(d: Dock) {
  const to: Dock = d === "right" ? "bottom" : "right", on = P.on[d];
  for (const id of docked()[d]) P.layout = moveTab(P.layout, id, to);
  for (const id of P.moved.keys()) if (docked()[d].includes(id)) P.moved.set(id, to);
  P.layout.dock = to;
  P.on[to] = on;
  saveLayout();
  draw();
}

/// Folds dock `d` to its strip, or unfolds it.
export function foldDock(d: Dock, folded: boolean) {
  P.layout.hidden[d] = folded;
  saveLayout();
  draw();
}
