// Draws the pane's docks: works out what each shows (its tab strip,
// controls and the view of each tab) for dock-view.tsx, and handles the
// clicks and the menu on its tabs.
import type { SubagentRow } from "./subagents-view.tsx";
import { chatAgentsCount } from "./chat-agents.tsx";
import type { PaneTabItem } from "./tabs-view.tsx";
import { hasPreview } from "./html-preview-state.ts";
import { type Dock, DOCKS } from "./media-state.ts";
import type { Artifact } from "./artifacts-state.ts";
import { artifactCount, artifactsTab, setArtifacts } from "./artifacts.ts";
import { changesTab } from "./changes.ts";
import { filesTab } from "./files.ts";
import { worktreeMenu } from "./worktree.ts";
import { drag } from "./drag.ts";
import { togglePreview } from "./text-view.tsx";
import type { IconName } from "../ui/icons.tsx";
import { runningCount } from "./running.ts";
import { followFiles } from "./reload.ts";
import { onShownChange } from "../ui/active.ts";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { openMenu, type MenuItem } from "../ui/menu.tsx";
import { writeClipboard } from "../ui/parts.tsx";
import { sessionsRead } from "../agents/sessions.ts";
import { type TabKind, AGENTS, ARTS, CHANGES, setFolded, closeTab, FILES, P, previewTab, RUNNING, DEVICE, saveFile, saveLayout, saveWorkFile, showTab, SUBS, type MediaTab, focusTab } from "./media.ts";
import { type DockModel, type DockView, chatPane, desktopChatPane, docked, foldNarrow, paneModel, paneOf, place, setPaneModel, shell } from "./media-layout.ts";

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
  foldNarrow();
  // A tab being dragged is not drawn over until it drops.
  if (drag?.ghost) return;
  const split = docked();
  for (const d of DOCKS) syncDock(d, split[d]);
  followFiles();
  publish(split);
}

sessionsRead.add(() => {
  if (shell() && desktopChatPane()) draw();
});

/// Shows the tab now in `P.on[d]` when nothing else changed: dock `d`'s views flip and
/// it draws again alone, without laying the docks out. False when the docks need `draw`.
export function flipTab(d: Dock) {
  const { docks, phone } = paneModel.getState();
  if (drag?.ghost || phone || !docks?.[d].shown || DOCKS.some(x => docks[x].zoomed !== (P.zoom === x))) return false;
  const ids = docked()[d];
  if (ids.length !== docks[d].tabs.length || ids.some((id, i) => docks[d].tabs[i].id !== id)) return false;
  syncDock(d, ids);
  followFiles();
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

/// On a phone the session shows one pane at a time: Chat, the conversation, or one of the
/// bottom dock's tabs filling the screen; mobile.css hides the dock's own strip.
function phoneTabs(ids: string[]) {
  return [{ id: "log", name: "Chat" }, ...ids.map(id => ({ id, name: paneTabItem(id).name }))];
}

/// A tab picked in the phone's strip.
export function pickPhoneTab(id: string) {
  setPaneModel({ log: id === "log" });
  if (id !== "log") P.on.bottom = id;
  draw();
}

/// The tab strip item of tab `id`.
export function paneTabItem(id: string): PaneTabItem {
  if (isFixed(id)) return { id, name: FIXED_NAMES[id], icon: TAB_ICONS[id], count: fixedCount(id), closeIcon: null };
  const tab = P.tabs.find(t => t.id === id);
  return { id, name: tab?.name ?? id, icon: TAB_ICONS[tab?.kind ?? "text"], count: null, closeIcon: "x" };
}

/// What dock `d` with tabs `ids` shows.
function dockModel(d: Dock, ids: string[]): DockModel {
  const on = P.on[d];
  const has = (id: string) => ids.includes(id);
  const views: DockView[] = [
    ...(has(FILES) ? [{ kind: "files" as const, hidden: on !== FILES }] : []),
    ...(has(CHANGES) && changesMade ? [{ kind: "changes" as const, hidden: on !== CHANGES }] : []),
    ...(has(ARTS) ? [{ kind: "artifacts" as const, hidden: on !== ARTS }] : []),
    ...(has(RUNNING) ? [{ kind: "running" as const, hidden: on !== RUNNING }] : []),
    ...(has(SUBS) ? [{ kind: "subagents" as const, hidden: on !== SUBS }] : []),
    ...(has(AGENTS) ? [{ kind: "agents" as const, hidden: on !== AGENTS }] : []),
    // A folded dock hides the device too, which stops its stream.
    ...(has(DEVICE) ? [{ kind: "device" as const, hidden: on !== DEVICE || P.layout.hidden[d] }] : []),
    ...P.tabs.filter(t => has(t.id)).map(t => ({ kind: "tab" as const, tab: t, rev: t.rev, hidden: t.id !== on })),
  ];
  const before = paneModel.getState().docks?.[d];
  return {
    tabs: ids.map(paneTabItem), on, views,
    shown: before?.shown ?? false, strip: before?.strip ?? false, size: before?.size ?? null, zoomed: before?.zoomed ?? false,
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

type Fixed = "subagents" | "agents" | "files" | "changes" | "artifacts" | "running" | "device";

const FIXED_NAMES: Record<Fixed, string> = { device: "Device", running: "Running", subagents: "Subagents", agents: "Agents", artifacts: "Artifacts", changes: "Changes", files: "Files" };

function isFixed(id: string): id is Fixed {
  return Object.hasOwn(FIXED_NAMES, id);
}

function fixedCount(id: Fixed) {
  if (id === SUBS) return P.subCount;
  if (id === RUNNING) return runningCount();
  if (id === ARTS) return artifactCount();
  if (id === AGENTS) { const chat = chatPane(); return chat ? chatAgentsCount(chat) : null; }
  return null;
}

const TAB_ICONS: Record<TabKind | Fixed, IconName> = {
  files: "paneFiles", changes: "paneChanges", text: "paneText", term: "paneTerm", session: "paneSubagents", image: "paneImage", model: "paneModel",
  video: "paneVideo", audio: "paneAudio", pdf: "panePdf", artifacts: "paneArtifacts", running: "paneRunning", subagents: "paneSubagents", agents: "paneSubagents", device: "paneDevice",
};

/// Pauses a tab's video or audio and its model's animation, whose frames would
/// otherwise keep drawing out of sight.
function stopPlaying(t: MediaTab) {
  t.player?.pause();
  t.model?.anim?.play(false);
}

/// Loads a tab's video again where it stood: WebKit can lose its picture
/// while it is out of sight and show it dark, sound and controls still there.
function showFrame(t: MediaTab) {
  const video = t.player;
  if (!(video instanceof HTMLVideoElement) || video.readyState === 0) return;
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
  if (t.kind === "session" && t.splitSession) {
    const split = t.splitSession;
    items.push({ label: "Open in main pane", on: () => {
      closeTab(t.id);
      void import("../agents/session.tsx").then(({ openAgent }) => openAgent({
        ...split, doing: null, started_at: 0, last_activity: 0, question: null, assistant: false, persona: null,
      }));
    } });
  }
  const path = t.kind === "text" ? t.rel : t.path;
  if (t.kind === "text" && t.rel && hasPreview(t.rel) && !t.text?.binary) items.push({ label: t.preview?.active ? "Source" : "Preview", on: () => { focusTab(t.id); if (t.preview?.active) void togglePreview(t); else previewTab(t); } });
  if (path) items.push({ label: "Copy path", on: () => void writeClipboard(path) });

  if (t.kind === "text" && t.rel) {
    const rel = t.rel;
    items.push({ label: "Save", on: () => saveWorkFile(rel, note, t.root) });
  } else if (t.path && P.session) {
    const { machine_id, id: session } = P.session, file = t.path;
    items.push({ label: "Save", on: () => saveFile("agent_file_save", { machineId: machine_id, path: file, session }, note) });
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
  // Moving the whole pane also places tabs that only appear in other sessions.
  P.layout.dock = to;
  P.layout.tabs = {};
  P.moved.clear();
  setFolded(to, false);
  P.on[to] = on;
  saveLayout();
  draw();
}

/// Folds dock `d` to its strip, or unfolds it.
export function foldDock(d: Dock, folded: boolean) {
  setFolded(d, folded);
  saveLayout();
  draw();
}
