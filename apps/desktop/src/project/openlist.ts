// The controls Home's and a project's Issues and Pull requests lists share:
// when it opened, newest or oldest, Select on issues, and Start N agents.
// Each list keeps its choices while the app runs; the callers draw the rows.
import { el as h } from "../ui/dom.ts";
import { dbtn as button } from "../ui/parts.ts";
import { agentLabel } from "./projects-state.ts";
import { issueStartPlan, loadStartHosts, startIssues } from "./composer.ts";
import { OPENED_IN, arrange, newChoice, pickedInOrder, startLabel, type ListChoice, type StartEntry, type StartRow } from "./openlist-state.ts";

const choices = new Map<string, ListChoice>();

const choiceOf = (key: string) => {
  let c = choices.get(key);
  if (!c) choices.set(key, c = newChoice());
  return c;
};

/// How the last Start N agents of a list ended.
interface Outcome {
  started: StartRow[];
  failed: StartRow | null;
  error: string | null;
  /// The rows left out, with why.
  left: StartRow[];
}

const outcomes = new Map<string, Outcome>();
const starting = new Set<string>();

/// `items` for list `key`, in the window and order picked there.
export function arranged<T>(key: string, items: readonly T[], opened: (item: T) => number | null | undefined) {
  const c = choiceOf(key);
  return arrange(items, opened, c.window, c.order, Date.now());
}

/// The row above a list: when it opened, the order, and Select for issues.
/// `redraw` runs on a choice.
export function listControls(key: string, redraw: () => void, issues: boolean) {
  const c = choiceOf(key);
  const seg = OPENED_IN.map(w => {
    const b = button(w.label, `quiet small${c.window === w.id ? " on" : ""}`, () => { c.window = w.id; redraw(); });
    b.setAttribute("aria-pressed", String(c.window === w.id));
    return b;
  });
  const order = button(c.order === "newest" ? "Newest first" : "Oldest first", "quiet small", () => { c.order = c.order === "newest" ? "oldest" : "newest"; redraw(); });
  const kids = [h("span", "seg", ...seg), h("span", "sp"), order];
  if (issues) {
    const select = button("Select", `quiet small${c.selecting ? " on" : ""}`, () => {
      c.selecting = !c.selecting;
      c.confirming = false;
      if (!c.selecting) c.picked.clear();
      else loadStartHosts().then(redraw).catch(e => console.warn("agent hosts:", e));
      redraw();
    });
    select.setAttribute("aria-pressed", String(c.selecting));
    kids.push(select);
  }
  return h("div", "pj-ctl", ...kids);
}

/// The checkbox of issue `id` while the list selects, else null.
export function pickBox(key: string, id: string, redraw: () => void) {
  const c = choiceOf(key);
  if (!c.selecting) return null;
  const box = h("input");
  box.type = "checkbox";
  box.checked = c.picked.has(id);
  box.setAttribute("aria-label", "Pick this issue");
  box.addEventListener("click", e => e.stopPropagation());
  box.addEventListener("change", () => {
    if (box.checked) c.picked.add(id); else c.picked.delete(id);
    c.confirming = false;
    redraw();
  });
  return h("label", "pick", box);
}

/// The line under a list that selects: how many are picked and Start N
/// agents, then the one confirm step with each issue's agent and machine, and
/// how the last start ended. `shown` is the list as drawn.
export function startBar(key: string, shown: readonly StartEntry[], redraw: () => void): HTMLElement | DocumentFragment {
  const c = choiceOf(key);
  const done = outcomes.get(key);
  const out = done ? outcomeEl(done, () => { outcomes.delete(key); redraw(); }) : null;
  if (!c.selecting) return out ?? document.createDocumentFragment();
  const picked = pickedInOrder(shown, c.picked);
  const busy = starting.has(key);
  if (!c.confirming) {
    const go = button(startLabel(picked.length), "pri small", () => { c.confirming = true; redraw(); });
    go.disabled = !picked.length;
    return h("div", "pj-start", h("span", "", `${picked.length} picked`), h("span", "sp"), go, out);
  }
  const plan = issueStartPlan(picked);
  if (!plan) return h("div", "pj-start", h("span", "", "Reading machines…"));
  const runnable = plan.filter(r => !r.skip);
  const lines = plan.map(r => h("span", r.skip !== null ? "skip" : "", `${r.entry.project ?? "No project"} #${r.entry.number} ${r.entry.title}`, r.skip !== null ? ` (${r.skip})` : ` on ${agentLabel(r.agent)}, ${r.machine_name}`));
  const go = button(busy ? "Starting…" : startLabel(runnable.length), "pri small", async () => {
    starting.add(key);
    redraw();
    const result = await startIssues(plan, () => redraw());
    starting.delete(key);
    outcomes.set(key, { ...result, left: plan.filter(r => r.skip) });
    c.picked.clear();
    c.confirming = false;
    c.selecting = !!result.error;
    redraw();
  });
  go.disabled = busy || !runnable.length;
  const cancel = button("Cancel", "quiet small", () => { c.confirming = false; redraw(); });
  cancel.disabled = busy;
  return h("div", "pj-start ask", h("b", "", `${startLabel(runnable.length)}?`),
    h("span", "", "One new worktree each, started one after another. Each runs on your plan."),
    ...lines, h("span", "acts", go, cancel));
}

/// What a finished start says: those started, the one that failed and why,
/// and those left out.
function outcomeEl(o: Outcome, dismiss: () => void) {
  const code = (r: StartRow) => `${r.entry.project ? `${r.entry.project} ` : ""}#${r.entry.number}`;
  const parts = [o.started.length ? `Started ${o.started.map(code).join(", ")}.` : "Nothing started."];
  if (o.failed) parts.push(`${code(o.failed)} failed: ${o.error}`);
  if (o.left.length) parts.push(`Left out: ${o.left.map(r => `${code(r)} (${r.skip})`).join(", ")}.`);
  return h("div", `pj-start${o.error ? " cc-err" : ""}`, h("span", "", parts.join(" ")), h("span", "sp"), button("Dismiss", "quiet small", dismiss));
}
