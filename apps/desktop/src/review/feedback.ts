import type { ReactElement } from "react";
import { flushSync } from "react-dom";
import type { FeedbackState } from "./feedback-view.tsx";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
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
  doneNote, findingsOf, mergeComments, reviewBase, reviewsOf, runState, toComments, whyNot,
} from "./feedback-state.ts";
import { loadDraft, saveDraft } from "./draftstore.ts";
import type { DiffFile, ReviewComment } from "./diff-state.ts";
import { isStarting } from "../agents/starting-state.ts";
import type { Picked } from "../agents/agentpicker.tsx";
import { byPreference } from "../agents/agentpick.ts";
import { pickPopover } from "../agents/pick-popover.ts";
import { agentLabel } from "../project/projects-state.ts";
import { reason } from "../ui/format.ts";
/// Calls a Rust command; the Tauri binding loads on first use, so a diff view can be made where there is none.
const invoke = async <T = unknown>(cmd: string, args?: Record<string, unknown>) => (await import("../ui/tauri.ts")).invoke<T>(cmd, args);


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
let lastPick: Pick<Picked, "agent" | "model" | "effort" | "fast"> | null = null;
// The agents state loads the first time a review is
// asked for, not with every diff.
let agents: typeof import("../agents/agents-state.ts") | null = null;
const hostsHeld = () => agents?.AG.hosts ?? null;

function readHosts(done: () => void) {
  void import("../agents/agents-state.ts").then(m => { agents = m; return m.ensureAgentHosts(); }).then(done);
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

/// The node pushes a run's state with every change; an update that does not hold it is read.
function schedule(run: Run) {
  run.unfollow ??= subscribeEach(topic("reviews", run.id), event => {
    const read = reviewsOf(event.data)?.find(r => r.id === run.id);
    return read ? settle(run, read) : tick(run);
  }, { machine: run.machineId });
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
  } catch (e) {
    console.warn("review not read:", e);
    return fail(run, `The review could not be read: ${reason(e)}`);
  }

  if (runs.get(run.key) !== run) return;
  return settle(run, read);
}

/// Takes the state of `run` the node says: a running review waits for the next update.
async function settle(run: Run, read: ReviewRun) {
  if (runs.get(run.key) !== run) return;
  const state = runState(read);
  if (state === "running") return;
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
  /// Draws again, as the diff changes.
  redraw: () => void;
  dispose: () => void;
}

const LABEL = "Review with agent";
const HINT = "Ask an agent to review these changes. It only reads. Pick a model to start";
const RUNNING_HINT = "Open the review. The reviewer only reads; its findings become comments here";

export function feedbackButton(o: FeedbackOpts): Feedback {
  let disposed = false;
  let starting = false;
  let readReviews = false;
  const picked: Picked = { agent: "", model: "", effort: null, fast: null, query: "" };
  const listeners = new Set<() => void>();
  let current: FeedbackSnapshot = { state: { kind: "ready", label: LABEL, hint: HINT, reason: "", open: false }, picker: null, note: "" };

  const host = () => (o.machine ? hostsHeld()?.find(h => h.machine_id === o.machine!.id) ?? null : null);
  const why = () => {
    const v = o.view();
    const c: CanStart = { machine: o.machine, root: v ? v.root : null, hosts: hostsHeld(), changes: !!v?.files.length };
    return whyNot(c);
  };
  const pop = pickPopover({
    picked,
    machine: () => { const h = host(); return h && o.machine ? { ...o.machine, agents: h.agents } : null; },
    everywhere: () => [...new Set((hostsHeld() ?? []).flatMap(m => m.agents))],
    allowed: () => null,
    // A review runs where the diff's checkout is.
    machines: () => null,
    opening: () => {
      if (disposed || runs.has(o.key)) return false;
      pickDefaults();
      return true;
    },
    start: () => void start(),
    draw: drawButton,
  });

  function pickDefaults() {
    const offered = host()?.agents ?? [];
    if (offered.includes(picked.agent)) return;
    const first = lastPick && offered.includes(lastPick.agent) ? lastPick : null;

    if (first) Object.assign(picked, { agent: first.agent, model: first.model, effort: first.effort, fast: first.fast });
    else {
      const agent = o.prefer && offered.includes(o.prefer) ? o.prefer : [...offered].sort(byPreference)[0] ?? "";
      Object.assign(picked, { agent, model: "", effort: null, fast: null });
    }
  }

  /// Draws the button again. Flushed, as the picker places itself right after
  /// it renders; not while React is committing, as when the header goes.
  function drawButton(sync = true) {
    if (disposed) return;
    const run = runs.get(o.key);
    const state: FeedbackState = run ? { kind: "running", label: "Reviewing", agent: agentLabel(run.agent), hint: RUNNING_HINT }
      : starting ? { kind: "starting" } : { kind: "ready", label: LABEL, hint: HINT, reason: why(), open: pop.open };
    current = { state, picker: pop.view, note: notes.get(o.key) ?? "" };
    const notify = () => { for (const listener of listeners) listener(); };
    if (sync) flushSync(notify); else notify();
  }

  async function start() {
    const v = o.view();
    const machine = o.machine;
    if (!v || !machine || starting || runs.has(o.key)) return;
    lastPick = { agent: picked.agent, model: picked.model, effort: picked.effort, fast: picked.fast };
    const uncommitted = o.uncommitted();
    starting = true;
    drawButton();

    try {
      const started = await invoke<ReviewRun>("agent_review_start", {
        machineId: machine.id, agent: picked.agent, cwd: v.root, base: reviewBase(v, uncommitted),
        parent: o.session ?? null, model: picked.model.trim() || null, effort: picked.effort, fast: picked.agent === "codex" ? picked.fast : null,
      });
      const load = o.load;
      follow(started, machine.id, v.root, () => load(uncommitted));
    } catch (e) {
      o.say(`Could not start the review: ${reason(e)}`);
    }

    starting = false;
    redraw();
  }

  function follow(read: ReviewRun, machineId: string, root: string, load: () => Promise<DiffFile[]>) {
    const run: Run = { key: o.key, id: read.id, machineId, agent: read.agent, root, load };
    notes.delete(o.key);
    runs.set(o.key, run);
    schedule(run);
  }

  /// The reviews the session asked for itself, not taken yet, become comments. `list` is what the
  /// node pushed, or the first read.
  async function takeSessionReviews(list: ReviewRun[]) {
    const machine = o.machine;
    if (!machine || disposed) return;
    const root = o.view()?.root ?? "";

    for (const read of list) {
      const state = runState(read);
      // One still running that this window no longer follows, after a reload
      // or a lost link, is followed again so the button shows and opens it.
      if (state === "running" && !starting && !runs.has(o.key)) {
        follow(read, machine.id, root || read.cwd, () => o.load(o.uncommitted()));
        redraw();
        continue;
      }

      if (state !== "done" || wasTaken(o.key, read.id)) continue;
      // One the button started is delivered by its own run.
      if (runs.get(o.key)?.id === read.id) continue;
      o.say(await deliver(o.key, read, root, () => o.load(o.uncommitted())));
    }
  }

  /// Reads the session's reviews once, for the first draw; the pushes carry them after that.
  function readSessionReviews() {
    const machine = o.machine;
    // A start has no machine to ask yet.
    if (!machine || !o.session || isStarting(o.session)) return;
    void invoke<ReviewRun[]>("agent_reviews", { machineId: machine.id, session: o.session })
      .catch(e => { console.warn("session reviews not read:", e); return [] as ReviewRun[]; })
      .then(takeSessionReviews);
  }

  // The node pushes every review it keeps; the session's own are the ones whose parent it is.
  const stopReviews = o.machine && o.session
    ? subscribeEach(topic("reviews"), event => {
      const list = reviewsOf(event.data)?.filter(r => r.parent === o.session);
      if (list?.length && o.session && !isStarting(o.session)) return takeSessionReviews(list);
    }, { machine: o.machine.id })
    : () => {};

  function onButton() {
    const run = runs.get(o.key);
    if (run) return openReviewer(run, o.machine?.name ?? "");
    if (!pop.open) readHosts(redraw);
    void pop.toggle();
  }

  function redraw() {
    if (disposed) return;
    if (!readReviews) { readReviews = true; readSessionReviews(); }
    drawButton();
    if (pop.open && !runs.has(o.key)) void pop.redraw();
    else if (pop.open) pop.close();
  }

  const attachment: Attached = {
    files: () => o.view()?.files ?? null,
    add: o.add,
    redraw,
    say: o.say,
  };
  attached.set(o.key, attachment);
  readHosts(redraw);
  // The agents module holds the hosts and takes a push first: it subscribes at load, so this
  // one comes after it.
  let stopHosts = () => {};
  void import("../agents/agents-state.ts").then(() => {
    if (!disposed) stopHosts = subscribeEach([topic("machines"), topic("accounts")], redraw);
  });
  redraw();
  return {
    snapshot: () => current,
    subscribe: listener => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    setBox: pop.setBox,
    setButton: pop.setButton,
    setSlot: pop.setSlot,
    click: onButton,
    dismiss: () => { notes.delete(o.key); drawButton(); },
    redraw,
    dispose: () => {
      if (disposed) return;
      pop.close();
      disposed = true;
      stopHosts();
      stopReviews();
      if (attached.get(o.key) === attachment) attached.delete(o.key);
    },
  };
}
