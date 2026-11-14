import type { ReactElement } from "react";
import { flushSync } from "react-dom";
import type { FeedbackState } from "./feedback-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
// Review with agent: the diff header's button, the start form's picker
// under it, and what follows a pick. A pick asks the node of the diff's
// checkout to start a temporary reviewer there, a child of the session whose
// changes the diff shows; it only reads, and hands its findings in with the
// submit_review tool, which become draft comments. The reviewer never shows
// as a session. A review is kept by diff id apart from the view, so closing
// the diff does not lose it. Reviews the session asked for itself (its
// `review` tool) become comments too, once.
import {
  type CanStart, type ReviewRun,
  doneNote, findingsOf, hostsAfterRead, mergeComments, reviewBase, runState, toComments, whyNot,
} from "./feedback-state.ts";
import { loadDraft, saveDraft } from "./draftstore.ts";
import type { DiffFile, ReviewComment } from "./diff-state.ts";
import type { AgentHost } from "../agents/agents.ts";
import type { Picked } from "../agents/agentpicker.ts";
import { keepEffort } from "../agents/agentpick.ts";
import { agentLabel } from "../project/projects-state.ts";
import { reason } from "../ui/format.ts";
/// Calls a Rust command; the Tauri binding loads on first use, so a diff view can be made where there is none.
const invoke = async <T = unknown>(cmd: string, args?: Record<string, unknown>) => (await import("../ui/tauri.ts")).invoke<T>(cmd, args);


// The sessions and picker modules pull in the whole session view, so they load
// the first time a review is asked for, not with every diff.
const agents = () => import("../agents/agents.ts");
const picker = () => import("../agents/agentpicker.ts");

/// How often a running review is read.

/// A review in progress.
interface Run {
  key: string;
  id: string;
  machineId: string;
  agent: string;
  root: string;
  /// Reads the diff again, for when the view that started it is closed.
  load: () => Promise<DiffFile[]>;
  /// Reads of the review that failed in a row.
  failed: number;
  unfollow?: () => void;
}

const runs = new Map<string, Run>();
/// Why a review ended without one, until it is dismissed.
const notes = new Map<string, string>();
/// The diff last made for each id: what a finished review is handed to. It
/// keeps the draft in storage as it changes, open or closed.
interface Attached {
  files: () => DiffFile[] | null;
  add: (comments: ReviewComment[]) => number;
  redraw: () => void;
  say: (text: string) => void;
}

const attached = new Map<string, Attached>();
/// The harness, model and thinking last picked, for the next time.
let lastPick: Pick<Picked, "agent" | "model" | "effort"> | null = null;
/// The machines that run coding agents, as last read.
let hosts: AgentHost[] | null = null;
/// Whether the last read failed, so an empty list does not mean offline.
let hostsFailed = false;
let hostsAsked = 0;
/// How long the machines read lasts.
const HOSTS_MS = 30_000;

function readHosts(done: () => void) {
  if (hosts && Date.now() - hostsAsked < HOSTS_MS) return;
  hostsAsked = Date.now();
  invoke<AgentHost[]>("agent_hosts")
    .then(list => { ({ hosts, failed: hostsFailed } = hostsAfterRead(hosts, list)); void picker().then(p => p.preloadModels(list)); done(); })
    .catch(e => { console.warn("agent hosts:", e); ({ hosts, failed: hostsFailed } = hostsAfterRead(hosts, null)); done(); });
}

// ----- Reviews already taken -----

const TAKEN_KEY = "beep.reviewsTaken";

/// The reviews whose findings became comments, by diff id, so a deleted
/// comment does not come back. Kept per viewer; lost storage only offers
/// them again.
function taken(): Record<string, string[]> {
  try {
    const got: unknown = JSON.parse(localStorage.getItem(TAKEN_KEY) || "{}");
    return got && typeof got === "object" ? got as Record<string, string[]> : {};
  } catch {
    return {};
  }
}

function wasTaken(key: string, id: string) {
  return (taken()[key] ?? []).includes(id);
}

function markTaken(key: string, id: string) {
  const all = taken();
  all[key] = [...(all[key] ?? []).filter(x => x !== id), id].slice(-50);

  try {
    localStorage.setItem(TAKEN_KEY, JSON.stringify(all));
  } catch {
    // Without storage the review is only offered again.
  }
}

// ----- Following a review -----

function schedule(run: Run) {
  run.unfollow ??= subscribeChanges(topic("reviews", run.id), { deliver: "latest", receive: () => tick(run) }, { machine: run.machineId });
}

function finish(run: Run) {
  run.unfollow?.();
  if (runs.get(run.key) === run) runs.delete(run.key);
  attached.get(run.key)?.redraw();
}

function fail(run: Run, text: string) {
  notes.set(run.key, text);
  finish(run);
}

async function tick(run: Run) {
  if (runs.get(run.key) !== run) return;
  let read: ReviewRun;

  try {
    read = await invoke<ReviewRun>("agent_review", { machineId: run.machineId, id: run.id });
    run.failed = 0;
  } catch (e) {
    console.warn("review not read:", e);
    if (++run.failed >= 5) return fail(run, `The review could not be read: ${reason(e)}`);
    throw e;
  }

  if (runs.get(run.key) !== run) return;
  const state = runState(read);
  if (state === "running") return schedule(run);
  if (state === "failed") return fail(run, read.error || "The review failed.");
  const n = await deliver(run.key, read, run.root, run.load);
  say(run.key, n);
  finish(run);
}

/// A submitted review's findings become draft comments, in the open diff or,
/// with none open, in the stored draft. Gives the line the header says.
async function deliver(key: string, read: ReviewRun, root: string, load: () => Promise<DiffFile[]>) {
  const review = read.review!;
  markTaken(key, read.id);
  const view = attached.get(key);
  let files = view?.files() ?? null;
  if (!files?.length) files = await load().catch(e => { console.warn("diff not read again:", e); return null; });
  const comments = toComments(findingsOf(review, root), files ?? [], read.agent);
  // A newer viewer may have replaced this one while the diff was loading.
  const current = attached.get(key);
  let added: number;
  if (current) added = current.add(comments);
  else {
    const merged = mergeComments(loadDraft(key), comments);
    saveDraft(key, merged.draft);
    added = merged.added;
  }
  return doneNote(agentLabel(read.agent), review.verdict, added, comments.length);
}

function say(key: string, text: string) {
  attached.get(key)?.say(text);
}

/// Opens the reviewer's conversation beside the changes it reviews.
/// Read when needed: the agent views need the window.
function openReviewer(run: Run, machineName: string) {
  void import("../bar/pane.ts").then(({ pane }) => pane.open({ agent: { machine_id: run.machineId, machine_name: machineName, id: run.id, agent: run.agent, cwd: run.root, status: "working", external: false, reviewer: true } }));
}

// ----- The button -----

interface FeedbackOpts {
  /// The diff's id: its drafts and its review are kept under it.
  key: string;
  /// The machine the diff's checkout is on; null for a diff with none.
  machine: { id: string; name: string } | null;
  /// The session whose changes the diff shows: the reviewer is its child.
  session?: string;
  /// The harness to offer first.
  prefer?: string;
  /// The diff as read: its checkout and its files; null before it is.
  view: () => { root: string; base: string; merge_base: string; files: DiffFile[] } | null;
  /// Whether the diff shows the uncommitted changes only.
  uncommitted: () => boolean;
  /// Reads the diff's files again.
  load: (uncommitted: boolean) => Promise<DiffFile[]>;
  /// Adds comments to the diff's draft; says how many were new.
  add: (comments: ReviewComment[]) => number;
  /// What the picker stays inside.
  bounds: () => HTMLElement;
  /// A short line in the diff's header.
  say: (text: string) => void;
}

/// What the header draws of a review: the button, the picker under it and why the last one failed.
interface FeedbackSnapshot { state: FeedbackState; picker: ReactElement | null; note: string }

export interface Feedback {
  snapshot: () => FeedbackSnapshot;
  subscribe: (listener: () => void) => () => void;
  /// The elements the view renders, for placing the picker and closing it.
  setBox: (box: HTMLElement | null) => void;
  setButton: (button: HTMLButtonElement | null) => void;
  setSlot: (slot: HTMLDivElement | null) => void;
  click: () => void;
  dismiss: () => void;
  /// Reads the session's own reviews and draws again, as the diff changes.
  redraw: () => void;
  dispose: () => void;
}

export function feedbackButton(o: FeedbackOpts): Feedback {
  let disposed = false;
  let box: HTMLElement | null = null;
  let button: HTMLButtonElement | null = null;
  let slot: HTMLDivElement | null = null;
  let pickerView: ReactElement | null = null;
  let open = false;
  let starting = false;
  let closePickerView: ((el: HTMLElement) => void) | null = null;
  /// When the session's own reviews were last read.
  let tookAt = 0;
  const picked: Picked = { agent: "", model: "", effort: null, query: "" };
  const listeners = new Set<() => void>();
  /// Set while the header unmounts, when React cannot flush.
  let committing = false;
  let current: FeedbackSnapshot = { state: { kind: "ready", reason: "", open: false }, picker: null, note: "" };
  const connected = () => !!box?.isConnected;

  const host = () => (o.machine ? hosts?.find(h => h.machine_id === o.machine!.id) ?? null : null);
  const why = () => {
    const v = o.view();
    const c: CanStart = { machine: o.machine, root: v ? v.root : null, hosts, hostsFailed, changes: !!v?.files.length };
    return whyNot(c);
  };

  function pickDefaults(byPreference: (a: string, b: string) => number) {
    const offered = host()?.agents ?? [];
    if (offered.includes(picked.agent)) return;
    const first = lastPick && offered.includes(lastPick.agent) ? lastPick : null;

    if (first) Object.assign(picked, { agent: first.agent, model: first.model, effort: first.effort });
    else {
      const agent = o.prefer && offered.includes(o.prefer) ? o.prefer : [...offered].sort(byPreference)[0] ?? "";
      Object.assign(picked, { agent, model: "", effort: null });
    }

    picked.query = "";
  }

  /// Draws the button again. Flushed, as the picker places itself right after
  /// it renders; not while React is committing, as when the header goes.
  function drawButton(sync = true) {
    if (disposed) return;
    const run = runs.get(o.key);
    const state: FeedbackState = run ? { kind: "running", agent: agentLabel(run.agent) }
      : starting ? { kind: "starting" } : { kind: "ready", reason: why(), open };
    current = { state, picker: pickerView, note: notes.get(o.key) ?? "" };
    const notify = () => { for (const listener of listeners) listener(); };
    if (sync) flushSync(notify); else notify();
  }

  function closePicker() {
    open = false;
    if (slot) {
      const closing = slot;
      closePickerView?.(closing);
    }
    document.removeEventListener("pointerdown", outside, true);
    document.removeEventListener("keydown", onEsc, true);
  }

  const outside = (e: Event) => {
    if (!connected() || !(e.target instanceof Node) || !box?.contains(e.target)) { closePicker(); drawButton(); }
  };
  const onEsc = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    // A diff taken off the page lets the key through.
    if (!connected()) { closePicker(); drawButton(); return; }
    e.preventDefault();
    e.stopPropagation();
    closePicker();
    drawButton();
    button?.focus();
  };

  async function drawPickerNow() {
    const h = host();
    if (!open || !h || !o.machine || !slot || !button || !connected()) return;
    const { drawPicker, focusSearch, closedPicker } = await picker();
    closePickerView = closedPicker;
    if (!open || !slot || !button || !connected()) return;
    const currentSlot = slot, currentButton = button;
    const focused = currentSlot.contains(document.activeElement);
    drawPicker(currentSlot, {
      render: view => { pickerView = view; drawButton(!committing); },
      picked,
      offered: h.agents,
      everywhere: [...new Set((hosts ?? []).flatMap(m => m.agents))],
      machine: o.machine,
      noMachine: "",
      anchor: () => currentButton,
      bounds: o.bounds,
      changed: what => {
        if (what === "model") { closePicker(); drawButton(); void start(); return; }
        if (what === "agent") picked.effort = keepEffort(picked.agent, picked.effort);
        if (what !== "models") void drawPickerNow();
        if (what === "agent") focusSearch(currentSlot);
      },
    });
    if (focused) focusSearch(currentSlot);
  }

  async function start() {
    const v = o.view();
    const machine = o.machine;
    if (!v || !machine || starting || runs.has(o.key)) return;
    lastPick = { agent: picked.agent, model: picked.model, effort: picked.effort };
    const uncommitted = o.uncommitted();
    starting = true;
    drawButton();

    try {
      const started = await invoke<ReviewRun>("agent_review_start", {
        machineId: machine.id, agent: picked.agent, cwd: v.root, base: reviewBase(v, uncommitted),
        parent: o.session ?? null, model: picked.model.trim() || null, effort: picked.effort,
      });
      const load = o.load;
      const run: Run = {
        key: o.key, id: started.id, machineId: machine.id, agent: picked.agent, root: v.root,
        load: () => load(uncommitted), failed: 0,
      };
      notes.delete(o.key);
      runs.set(o.key, run);
      schedule(run);
    } catch (e) {
      o.say(`Could not start the review: ${reason(e)}`);
    }

    starting = false;
    redraw();
  }

  /// The reviews the session asked for itself, not taken yet, become comments.
  async function takeSessionReviews() {
    const machine = o.machine;
    if (!machine || !o.session) return;
    const list = await invoke<ReviewRun[]>("agent_reviews", { machineId: machine.id, session: o.session })
      .catch(e => { console.warn("session reviews not read:", e); return [] as ReviewRun[]; });
    const root = o.view()?.root ?? "";

    for (const read of list) {
      if (runState(read) !== "done" || wasTaken(o.key, read.id)) continue;
      // One the button started is delivered by its own run.
      if (runs.get(o.key)?.id === read.id) continue;
      o.say(await deliver(o.key, read, root, () => o.load(o.uncommitted())));
    }
  }

  async function onButton() {
    if (!button || button.disabled) return;
    const run = runs.get(o.key);
    if (run) return openReviewer(run, o.machine?.name ?? "");
    if (open) { closePicker(); return drawButton(); }
    readHosts(redraw);
    const [a, p] = await Promise.all([agents(), picker()]);
    if (disposed || runs.has(o.key) || open || !connected()) return;
    pickDefaults(a.byPreference);
    open = true;
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", onEsc, true);
    drawButton();
    await drawPickerNow();
    if (slot) p.focusSearch(slot);
  }

  function redraw() {
    if (disposed) return;
    if (Date.now() - tookAt >= HOSTS_MS) { tookAt = Date.now(); void takeSessionReviews(); }
    if (open && !connected()) closePicker();
    drawButton();
    if (open && !runs.has(o.key)) void drawPickerNow();
    else if (open) closePicker();
  }

  const attachment: Attached = {
    files: () => o.view()?.files ?? null,
    add: o.add,
    redraw,
    say: o.say,
  };
  attached.set(o.key, attachment);
  readHosts(redraw);
  redraw();
  return {
    snapshot: () => current,
    subscribe: listener => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    setBox: value => {
      box = value;
      // The header went with the picker open.
      if (!value && open) { committing = true; closePicker(); committing = false; drawButton(false); }
    },
    setButton: value => { button = value; },
    setSlot: value => { slot = value; },
    click: () => void onButton(),
    dismiss: () => { notes.delete(o.key); drawButton(); },
    redraw,
    dispose: () => {
      if (disposed) return;
      closePicker();
      disposed = true;
      if (attached.get(o.key) === attachment) attached.delete(o.key);
    },
  };
}
