import { agentLabel } from "../../stores/project/projects-state.ts";
import { choiceOf, outcomes, starting, type Outcome } from "../../stores/project/openlist.ts";
import { OPENED_IN, pickedInOrder, startLabel, type StartEntry, type StartRow } from "../../stores/project/openlist-state.ts";
import { Button } from "../../ui/kit.tsx";

export function OpenListControls({ list, redraw, issues, loadHosts, flexItem = false }: { list: string; redraw(): void; issues: boolean; loadHosts(): Promise<void>; flexItem?: boolean }) {
  const choice = choiceOf(list);
  return <div className="pj-ctl" style={flexItem ? { flex: "none" } : undefined}><span className="seg">{OPENED_IN.map(window => <Button key={window.id} look="quiet" small className={choice.window === window.id ? "on" : ""} aria-pressed={choice.window === window.id} onClick={() => { choice.window = window.id; redraw(); }}>{window.label}</Button>)}</span><span className="sp" /><Button look="quiet" small onClick={() => { choice.order = choice.order === "newest" ? "oldest" : "newest"; redraw(); }}>{choice.order === "newest" ? "Newest first" : "Oldest first"}</Button>{issues && <Button look="quiet" small className={choice.selecting ? "on" : ""} aria-pressed={choice.selecting} onClick={() => { choice.selecting = !choice.selecting; choice.confirming = false; if (!choice.selecting) choice.picked.clear(); else void loadHosts().then(redraw).catch(error => console.warn("agent hosts:", error)); redraw(); }}>Select</Button>}</div>;
}
function Result({ outcome, dismiss, flexItem = false }: { outcome: Outcome; dismiss(): void; flexItem?: boolean }) {
  const code = (row: StartRow) => `${row.entry.project ? `${row.entry.project} ` : ""}#${row.entry.number}`;
  const text = [outcome.started.length ? `Started ${outcome.started.map(code).join(", ")}.` : "Nothing started."];
  if (outcome.failed) text.push(`${code(outcome.failed)} failed: ${outcome.error}`);
  if (outcome.left.length) text.push(`Left out: ${outcome.left.map(row => `${code(row)} (${row.skip})`).join(", ")}.`);
  return <div className={`pj-start${outcome.error ? " cc-err" : ""}`} style={flexItem ? { flex: "none" } : undefined}><span>{text.join(" ")}</span><span className="sp" /><Button look="quiet" small onClick={dismiss}>Dismiss</Button></div>;
}
export function OpenListStart({ list, shown, redraw, plan: planFor, run, flexItem = false }: { list: string; shown: readonly StartEntry[]; redraw(): void; plan(entries: readonly StartEntry[]): StartRow[] | null; run(rows: readonly StartRow[], done: (started: StartRow[]) => void): Promise<Outcome>; flexItem?: boolean }) {
  const choice = choiceOf(list), done = outcomes.get(list), busy = starting.has(list);
  const result = done ? <Result flexItem={flexItem} outcome={done} dismiss={() => { outcomes.delete(list); redraw(); }} /> : null;
  if (!choice.selecting) return result;
  const picked = pickedInOrder(shown, choice.picked);
  if (!choice.confirming) return <div className="pj-start" style={flexItem ? { flex: "none" } : undefined}><span>{picked.length} picked</span><span className="sp" /><Button look="primary" small disabled={!picked.length} onClick={() => { choice.confirming = true; redraw(); }}>{startLabel(picked.length)}</Button>{result}</div>;
  const plan = planFor(picked);
  if (!plan) return <div className="pj-start" style={flexItem ? { flex: "none" } : undefined}><span>Reading machines…</span></div>;
  const runnable = plan.filter(row => !row.skip);
  const start = async () => {
    if (starting.has(list)) return;
    starting.add(list); redraw();
    const outcome = await run(plan, redraw);
    starting.delete(list); outcomes.set(list, outcome);
    choice.picked.clear(); choice.confirming = false; choice.selecting = !!outcome.error; redraw();
  };
  return <div className="pj-start ask" style={flexItem ? { flex: "none" } : undefined}><b>{startLabel(runnable.length)}?</b><span>One new worktree each, started one after another. Each runs on your plan.</span>{plan.map(row => <span key={row.entry.key} className={row.skip !== null ? "skip" : ""}>{row.entry.project ?? "No project"} #{row.entry.number} {row.entry.title}{row.skip !== null ? ` (${row.skip})` : ` on ${agentLabel(row.agent)}, ${row.machine_name}`}</span>)}<span className="acts"><Button look="primary" small disabled={busy || !runnable.length} onClick={() => { void start(); }}>{busy ? "Starting…" : startLabel(runnable.length)}</Button><Button look="quiet" small disabled={busy} onClick={() => { choice.confirming = false; redraw(); }}>Cancel</Button></span></div>;
}
