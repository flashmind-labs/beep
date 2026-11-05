// Feedback from an agent: the diff header's button, the start form's picker
// under it, and what follows a pick. A pick starts a coding agent in the
// diff's checkout on its machine, held until it is in a mode that does not
// edit; it reads the diff with git and answers with a list of findings, which
// become draft comments when its turn ends. A review is kept by diff id apart
// from the view, so closing the diff does not lose it.
import {
  type CanStart, type ReviewTarget, type Stage,
  answerNote, answerOf, doneNote, holdPrompt, nextStep, parseFindings, readOnlyMode, reviewPrompt, reviewTarget, toComments, whyNot,
} from "./feedback-state.ts";
import type { DiffFile, ReviewComment } from "./diff-state.ts";
import type { AgentHost, AgentModes, AgentPage, AgentSession, Started } from "../agents/agents.ts";
import type { Picked } from "../agents/agentpicker.ts";
import { keepEffort } from "../agents/agentpick.ts";
import { agentLabel } from "../project/projects-state.ts";
import { el } from "../ui/dom.ts";
import { reason } from "../ui/format.ts";
import { harnessEl } from "../ui/harnessicon.ts";
/// Calls a Rust command; the Tauri binding loads on first use, so a diff view can be made where there is none.
const invoke = async <T = unknown>(cmd: string, args?: Record<string, unknown>) => (await import("../ui/tauri.ts")).invoke<T>(cmd, args);

const CARET = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';

// The sessions and picker modules pull in the whole session view, so they load
// the first time a review is asked for, not with every diff.
const agents = () => import("../agents/agents.ts");
const picker = () => import("../agents/agentpicker.ts");

/// A review in progress.
interface Run {
  key: string;
  session: AgentSession;
  agent: string;
  target: ReviewTarget;
  root: string;
  stage: Stage;
  /// The read-only mode it was switched to, once asked for; "" when the harness has none.
  mode: string | null;
  /// Reads the diff again, for when the view that started it is closed.
  load: () => Promise<DiffFile[]>;
  /// Reads of the session that failed in a row.
  failed: number;
  /// What the session last was, and since when.
  status: string;
  since: number;
}

/// A review's answer that could not be used, until it is dismissed.
interface Note {
  text: string;
  session: AgentSession | null;
}

/// The diff last made for each id: what a finished review is handed to. It
/// keeps the draft in storage as it changes, open or closed.
interface Attached {
  files: () => DiffFile[] | null;
  add: (comments: ReviewComment[]) => number;
  redraw: () => void;
}

const runs = new Map<string, Run>();
const notes = new Map<string, Note>();
const attached = new Map<string, Attached>();
/// The harness, model and thinking last picked, for the next time.
let lastPick: Pick<Picked, "agent" | "model" | "effort"> | null = null;
/// The machines that run coding agents, as last read.
let hosts: AgentHost[] | null = null;
let hostsAsked = 0;
/// How long the machines read lasts.
const HOSTS_MS = 30_000;

function readHosts(done: () => void) {
  if (hosts && Date.now() - hostsAsked < HOSTS_MS) return;
  hostsAsked = Date.now();
  invoke<AgentHost[]>("agent_hosts")
    .then(list => { hosts = list; void picker().then(p => p.preloadModels(list)); done(); })
    .catch(e => { console.warn("agent hosts:", e); hosts ??= []; done(); });
}

const sessionRef = (s: AgentSession) => ({ machineId: s.machine_id, agent: s.agent, session: s.id });

// ----- Following a review -----

/// Reads the session again after the session view's own wait, slower once it has been quiet.
async function schedule(run: Run) {
  const a = await agents();
  const still = Date.now() - run.since;
  window.setTimeout(() => tick(run), still >= a.AGENT_SLOW_AFTER ? a.AGENT_SLOW_MS : a.AGENT_POLL_MS);
}

function finish(run: Run) {
  if (runs.get(run.key) === run) runs.delete(run.key);
  attached.get(run.key)?.redraw();
}

function fail(run: Run, text: string) {
  notes.set(run.key, { text, session: run.session });
  finish(run);
}

/// Closes a review session that could not be made read only.
function closeSession(run: Run) {
  invoke("agent_steer", { ...sessionRef(run.session), steer: "close" }).catch(e => console.warn("review session not closed:", e));
}

async function tick(run: Run) {
  if (runs.get(run.key) !== run) return;
  let page: AgentPage;
  try {
    page = await invoke<AgentPage>("agent_view", { ...sessionRef(run.session), last: 8 });
    run.failed = 0;
  } catch (e) {
    console.warn("review session not read:", e);
    if (++run.failed >= 5) return finish(run);
    return schedule(run);
  }
  if (runs.get(run.key) !== run) return;
  const status = page.session.status;
  if (status !== run.status) { run.status = status; run.since = Date.now(); }
  const step = nextStep(run.stage, { status, turns: page.turns, asking: !!page.session.question });
  if (step === "gone") return finish(run);
  if (step === "asked") return fail(run, "The agent asked a question instead of answering. Open the session to answer it.");
  if (step === "wait") return schedule(run);
  if (step === "send") {
    try {
      if (await sendReview(run)) run.stage = "review";
    } catch (e) {
      return fail(run, `Could not send the review request: ${reason(e)}`);
    }
    return schedule(run);
  }
  await deliver(run, answerOf(page));
}

/// The hold turn ended: puts the session in its read-only mode, checks it took,
/// then sends the request. False until the mode shows.
async function sendReview(run: Run) {
  const at = sessionRef(run.session);
  if (run.mode === null) {
    const modes = await invoke<AgentModes | null>("agent_modes", at).catch(() => null);
    const want = modes ? readOnlyMode(modes.modes) : null;
    run.mode = want ?? "";
    if (want) {
      await invoke("agent_steer", { ...at, steer: { mode: { mode: want } } });
      return false;
    }
  }
  if (run.mode) {
    const modes = await invoke<AgentModes | null>("agent_modes", at).catch(() => null);
    if (modes?.current !== run.mode) {
      closeSession(run);
      throw new Error("the session did not go into its read-only mode, so it was closed");
    }
  }
  await invoke("agent_steer", { ...at, steer: { prompt: { text: reviewPrompt(run.target, run.root, !!run.mode) } } });
  return true;
}

/// The review's turn ended: its findings become draft comments, in the open
/// diff or, with none open, in the stored draft.
async function deliver(run: Run, answer: string) {
  const name = agentLabel(run.agent);
  const parsed = parseFindings(answer, run.root);
  if (!parsed) return fail(run, answerNote(answer));
  // The last diff that showed this review takes the comments, open or not: it keeps the draft.
  const view = attached.get(run.key);
  let files = view?.files() ?? null;
  if (!files?.length) files = await run.load().catch(e => { console.warn("diff not read again:", e); return null; });
  const comments = toComments(parsed.findings, files ?? [], run.agent);
  const added = view ? view.add(comments) : 0;
  const skipped = parsed.skipped ? `, ${parsed.skipped} unusable` : "";
  say(run.key, `${doneNote(name, added, comments.length)}${skipped}`);
  finish(run);
}

const saying = new Map<string, (text: string) => void>();
function say(key: string, text: string) {
  saying.get(key)?.(text);
}

// ----- The button -----

export interface FeedbackOpts {
  /// The diff's id: its drafts and its review are kept under it.
  key: string;
  /// The machine the diff's checkout is on; null for a diff with none.
  machine: { id: string; name: string } | null;
  /// The harness to offer first.
  prefer?: string;
  /// Opens a review's session.
  open: (session: AgentSession) => void;
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

export interface Feedback {
  /// The button and its picker, for the header.
  el: HTMLElement;
  /// A review's answer that could not be used, for under the header.
  note: HTMLElement;
  /// Draws both again, as the diff changes.
  redraw: () => void;
}

export function feedbackButton(o: FeedbackOpts): Feedback {
  const box = el("span", "dv-fbw");
  const slot = el("div");
  const note = el("div", "dv-fbnote");
  note.hidden = true;
  const button = el("button", "dv-btn dv-fb");
  button.type = "button";
  box.append(button, slot);
  let open = false;
  let starting = false;
  const picked: Picked = { agent: "", model: "", effort: null, query: "" };

  const host = () => (o.machine ? hosts?.find(h => h.machine_id === o.machine!.id) ?? null : null);
  const why = () => {
    const v = o.view();
    const c: CanStart = { machine: o.machine, root: v ? v.root : null, hosts, changes: !!v?.files.length };
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

  function drawButton() {
    const run = runs.get(o.key);
    button.replaceChildren();
    button.removeAttribute("aria-expanded");
    button.disabled = false;
    if (run) {
      button.append(el("span", "dspin"), "Reviewing ", el("small", "", agentLabel(run.agent)));
      button.title = "Open the review session";
      return;
    }
    if (starting) {
      button.append(el("span", "dspin"), "Starting");
      button.disabled = true;
      button.title = "";
      return;
    }
    const reasonNot = why();
    button.append("Feedback from an agent");
    const caret = el("span");
    caret.style.display = "grid";
    caret.innerHTML = CARET;
    button.append(caret);
    button.disabled = !!reasonNot;
    button.title = reasonNot || "Ask an agent to review these changes. It only reads. Pick a model to start";
    if (!reasonNot) button.setAttribute("aria-expanded", String(open));
  }

  function drawNote() {
    const n = notes.get(o.key);
    note.hidden = !n;
    note.replaceChildren();
    if (!n) return;
    const pre = el("pre", "", n.text);
    const actions = el("div", "a");
    if (n.session) {
      const s = n.session;
      const link = el("button", "dv-btn", "Open the session");
      link.type = "button";
      link.addEventListener("click", () => o.open(s));
      actions.append(link);
    }
    const dismiss = el("button", "dv-btn", "Dismiss");
    dismiss.type = "button";
    dismiss.addEventListener("click", () => { notes.delete(o.key); drawNote(); });
    actions.append(dismiss);
    note.append(el("b", "", "The review did not end in a list of comments"), pre, actions);
    note.setAttribute("role", "status");
  }

  function closePicker() {
    open = false;
    slot.replaceChildren();
    void picker().then(p => p.closedPicker(slot));
    document.removeEventListener("pointerdown", outside, true);
    document.removeEventListener("keydown", onEsc, true);
  }

  const outside = (e: Event) => {
    if (!box.contains(e.target as Node)) { closePicker(); drawButton(); }
  };
  const onEsc = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    closePicker();
    drawButton();
    button.focus();
  };

  async function drawPickerNow() {
    const h = host();
    if (!open || !h || !o.machine) return;
    const { drawPicker, focusSearch } = await picker();
    if (!open) return;
    const focused = slot.contains(document.activeElement);
    drawPicker(slot, {
      picked,
      offered: h.agents,
      everywhere: [...new Set((hosts ?? []).flatMap(m => m.agents))],
      machine: o.machine,
      noMachine: "",
      anchor: () => button,
      bounds: o.bounds,
      changed: what => {
        if (what === "model") { closePicker(); drawButton(); void start(); return; }
        if (what === "agent") picked.effort = keepEffort(picked.agent, picked.effort);
        if (what !== "models") void drawPickerNow();
        if (what === "agent") focusSearch(slot);
      },
    });
    if (focused) focusSearch(slot);
  }

  async function start() {
    const v = o.view();
    const machine = o.machine;
    if (!v || !machine || starting || runs.has(o.key)) return;
    lastPick = { agent: picked.agent, model: picked.model, effort: picked.effort };
    const target = reviewTarget(v, o.uncommitted());
    const uncommitted = o.uncommitted();
    starting = true;
    drawButton();
    try {
      const started = await invoke<Started>("agent_start", {
        machineId: machine.id, agent: picked.agent, cwd: v.root, prompt: holdPrompt(target),
        chat: null, model: picked.model.trim() || null, effort: picked.effort, checkout: null,
      });
      const run: Run = {
        key: o.key, session: started.session, agent: picked.agent, target, root: v.root, stage: "hold", mode: null,
        load: () => o.load(uncommitted), failed: 0, status: started.session.status, since: Date.now(),
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

  button.addEventListener("click", async e => {
    e.stopPropagation();
    const run = runs.get(o.key);
    if (run) return o.open(run.session);
    if (button.disabled) return;
    if (open) { closePicker(); return drawButton(); }
    readHosts(redraw);
    const [a, p] = await Promise.all([agents(), picker()]);
    if (runs.has(o.key) || open) return;
    pickDefaults(a.byPreference);
    open = true;
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", onEsc, true);
    drawButton();
    await drawPickerNow();
    p.focusSearch(slot);
  });

  function redraw() {
    drawButton();
    drawNote();
    if (open && !runs.has(o.key)) void drawPickerNow();
    else if (open) closePicker();
  }

  attached.set(o.key, {
    files: () => o.view()?.files ?? null,
    add: o.add,
    redraw,
  });
  saying.set(o.key, o.say);
  readHosts(redraw);
  redraw();
  return { el: box, note, redraw };
}
