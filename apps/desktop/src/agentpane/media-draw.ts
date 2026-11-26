// Draws the pane's docks: works out what each shows (its tab strip,
// controls and the view of each tab) for dock-view.tsx.
import type { SubagentRow } from "./subagents-view.tsx";
import { chatAgentsCount } from "./chat-agents.tsx";
import { type Dock, DOCKS, AGENTS, ARTS, CHANGES, DEVICE, FILES, RUNNING, SUBS } from "./media-state.ts";
import type { Artifact } from "./artifacts-state.ts";
import { artifactCount, artifactsTab, setArtifacts } from "./artifacts.ts";
import { changesTab } from "./changes.ts";
import { filesTab } from "./files.ts";
import { worktreeMenu } from "./worktree.ts";
import { drag } from "./drag.ts";
import { runningCount } from "./running.ts";
import { followFiles } from "./reload.ts";
import { onShownChange } from "../ui/active.ts";
import { refreshTopicSubscriptions } from "../ui/mesh-events.ts";
import { sessionsRead } from "../agents/sessions.ts";
import { P, paneAsks, type MediaTab } from "./pane-state.ts";
import { paneTabItem } from "./pane-tabs.ts";
import { hasDeviceTab } from "./device.ts";
import { hasFolderEntries } from "./files.ts";
import { type DockModel, type DockView, type PaneCounts, type PaneModel, chatPane, desktopChatPane, docked, foldNarrow, paneModel, paneOf, drawPaneNow, place, setOn, setPaneModel, shell } from "./media-layout.ts";

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
  if (subs) paneModel.setState({ subs: subs.rows, subCount: subs.count });
  if (arts) setArtifacts(arts.list, arts.turnStart);
  draw();
}

/// Reads what the fixed tabs count from their owners into the model, which the
/// docks lay out by, and gives it.
export function syncCounts(): PaneCounts {
  const chat = chatPane();
  const was = paneModel.getState().counts;
  const next = { running: runningCount(), artifacts: artifactCount(), agents: chat ? chatAgentsCount(chat) : null, device: hasDeviceTab() };
  const same = was.running === next.running && was.artifacts === next.artifacts && was.agents === next.agents && was.device === next.device;
  const counts = same ? was : next;
  paneModel.setState({ counts, entries: hasFolderEntries() });
  return counts;
}

/// Brings each dock's views up to date with what they show, then lays the docks out.
/// What the docks draw comes from the model; this works out what the model cannot read.
export function draw() {
  foldNarrow();
  const counts = syncCounts();
  // A tab being dragged is not drawn over until it drops.
  if (drag?.ghost) return;
  const split = docked();
  for (const d of DOCKS) syncDock(d, split[d]);
  followFiles();

  // The center window's phone layout, which center/mobile.ts marks on the body.
  const phone = shell() && document.body.classList.contains("mobile") ? phoneTabs(split.bottom) : null;
  // On a phone the session first shows the conversation.
  if (phone && !paneModel.getState().phoneShown) paneModel.setState({ phoneShown: true, log: true });
  const { log, on } = paneModel.getState();
  place({
    split, worktrees: worktreeMenu(),
    counts,
    phone: phone && { tabs: phone, selected: log ? "log" : on.bottom },
  });
}

// The tabs ask for a draw, and for the files they show to be followed.
paneAsks.subscribe((now, was) => { if (now.draws !== was.draws) draw(); });

sessionsRead.add(() => {
  if (shell() && desktopChatPane()) draw();
});

/// Shows the tab now on screen in dock `d` when nothing else changed: dock `d`'s views flip,
/// without laying the docks out. False when the docks need `draw`.
export function flipTab(d: Dock) {
  const { split, places, phone, zoom, zoomed } = paneModel.getState();
  if (drag?.ghost || phone || !split || !places[d].shown || zoomed !== zoom) return false;
  const ids = docked()[d];
  if (ids.length !== split[d].length || ids.some((id, i) => split[d][i] !== id)) return false;
  syncDock(d, ids);
  followFiles();
  // Draws the flipped views now: what syncDock started reading lands in a view that shows.
  setPaneModel({});
  drawPaneNow();
  refreshTopicSubscriptions();
  return true;
}

/// On a phone the session shows one pane at a time: Chat, the conversation, or one of the
/// bottom dock's tabs filling the screen; mobile.css hides the dock's own strip.
function phoneTabs(ids: string[]) {
  return [{ id: "log", name: "Chat" }, ...ids.map(id => ({ id, name: paneTabItem(id).name }))];
}

/// A tab picked in the phone's strip.
export function pickPhoneTab(id: string) {
  paneModel.setState({ log: id === "log" });
  if (id !== "log") setOn("bottom", id);
  draw();
}

/// What dock `d` shows, worked out from the model. The same model gives the same dock.
const docks: Record<Dock, { from: unknown[]; model: DockModel }> = { right: { from: [], model: { tabs: [], views: [], worktrees: null } }, bottom: { from: [], model: { tabs: [], views: [], worktrees: null } } };

export function dockModel(s: PaneModel, d: Dock): DockModel | null {
  if (!s.split) return null;
  const ids = s.split[d], on = s.on[d];
  const from = [s.tabs, s.subCount, s.counts, ids, on, s.layout.hidden[d], s.changes, s.worktrees];
  const had = docks[d];
  if (had.from.length === from.length && from.every((x, i) => Object.is(x, had.from[i]))) return had.model;

  const has = (id: string) => ids.includes(id);
  const views: DockView[] = [
    ...(has(FILES) ? [{ kind: "files" as const, hidden: on !== FILES }] : []),
    ...(has(CHANGES) && s.changes ? [{ kind: "changes" as const, hidden: on !== CHANGES }] : []),
    ...(has(ARTS) ? [{ kind: "artifacts" as const, hidden: on !== ARTS }] : []),
    ...(has(RUNNING) ? [{ kind: "running" as const, hidden: on !== RUNNING }] : []),
    ...(has(SUBS) ? [{ kind: "subagents" as const, hidden: on !== SUBS }] : []),
    ...(has(AGENTS) ? [{ kind: "agents" as const, hidden: on !== AGENTS }] : []),
    // A folded dock hides the device too, which stops its stream.
    ...(has(DEVICE) ? [{ kind: "device" as const, hidden: on !== DEVICE || s.layout.hidden[d] }] : []),
    ...s.tabs.filter(t => has(t.id)).map(t => ({ kind: "tab" as const, tab: t, rev: t.rev, hidden: t.id !== on })),
  ];
  const model = { tabs: ids.map(id => paneTabItem(id, s)), views, worktrees: on === FILES || on === CHANGES ? s.worktrees : null };
  docks[d] = { from, model };

  return model;
}

/// Brings the views of dock `d`, with tabs `ids`, up to date with what it shows.
function syncDock(d: Dock, ids: string[]) {
  // The tab on screen went: the newest one here.
  if (!ids.includes(paneModel.getState().on[d])) setOn(d, ids.at(-1) ?? FILES);
  const { on: shownTabs, layout } = paneModel.getState(), on = shownTabs[d], folded = layout.hidden[d];
  if (ids.includes(FILES)) filesTab(on === FILES, on === FILES && !folded);
  // The Changes view is made the first time it shows, as it runs git; shown
  // again, it reads only when the folder changed.
  if (ids.includes(CHANGES)) {
    const changes = changesTab(on === CHANGES, on === CHANGES && !folded);
    if (changes !== paneModel.getState().changes) paneModel.setState({ changes });
  }
  if (ids.includes(ARTS)) {
    const source = paneModel.getState().tabs.find(t => t.id === on)?.exported?.source;
    artifactsTab(on === ARTS, on === ARTS && !folded, !folded && source?.tab === ARTS ? source.url : null);
  }

  for (const t of paneModel.getState().tabs) {
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
      const now = paneModel.getState();
      if (t.id === now.on[d] && !now.layout.hidden[d] && pane && (document.activeElement === pane || t.term?.screen.focused())) t.term?.screen.focus();
    });
  }
}

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
  for (const t of paneModel.getState().tabs) {
    if (!shown) stopPlaying(t);
    else if (!t.viewHidden) showFrame(t);
  }
}, true);
