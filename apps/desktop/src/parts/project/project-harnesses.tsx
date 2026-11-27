// A project's Harnesses sheet: which coding agents may run in its checkouts on every machine.
import { useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../../ui/parts.tsx";
import { invoke } from "../../ui/tauri.ts";
import { reason } from "../../ui/format.ts";
import { Button, ChipToggle, SheetHead } from "../../ui/kit.tsx";
import { HARNESS_SLUGS, harnessName } from "../../stores/settings/providers-state.ts";
import type { Allowed as AllowedList } from "../../stores/agents/agentpick.ts";

/// The harnesses a project allows: any, or the ones picked.
type Allowed = { kind: "any" } | { kind: "only"; agents: readonly string[] };

/// Each project's allowed harnesses as last read, for the model pickers.
const ALLOWED = new Map<string, AllowedList>();

/// Reads which harnesses `project` allows; a folder Beep has not imported allows every one.
export function readProjectHarnesses(project: string): Promise<AllowedList> {
  return invoke<string[] | null>("project_harnesses", { project })
    .catch(() => null)
    .then(list => { ALLOWED.set(project, list); return list; });
}

/// The harnesses `project` allows as last read; null for every one.
export const projectHarnesses = (project: string | null): AllowedList => project ? ALLOWED.get(project) ?? null : null;

export function openProjectHarnesses(project: string) {
  let busy = false;
  const { close, render } = openSheet(`Harnesses in ${project}`, "am-sheet", { canClose: () => !busy });
  const head = <SheetHead title={`Harnesses in ${project}`} />;
  const save = async (allowed: Allowed) => {
    busy = true;
    try {
      const harnesses = allowed.kind === "any" ? null : allowed.agents;
      await invoke("project_harnesses_save", { project, harnesses });
      ALLOWED.set(project, harnesses);
      busy = false;
      close();
    } catch (error) {
      busy = false;
      throw error;
    }
  };

  flushSync(() => render(<>{head}<div className="dmuted">Loading…</div></>));
  invoke<string[] | null>("project_harnesses", { project }).then(
    saved => flushSync(() => render(<ProjectHarnessesView head={head} saved={saved ? { kind: "only", agents: saved } : { kind: "any" }} save={save} cancel={() => { if (!busy) close(); }} />)),
    error => flushSync(() => render(<>{head}<div className="am-msg bad" role="status">{reason(error)}</div><div className="am-line ip-acts"><Button look="quiet" onClick={close}>Close</Button></div></>)),
  );
}

function ProjectHarnessesView({ head, saved, save, cancel }: { head: ReactNode; saved: Allowed; save(allowed: Allowed): Promise<void>; cancel(): void }) {
  const [allowed, setAllowed] = useState(saved);
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const shown = [...HARNESS_SLUGS, ...(saved.kind === "only" ? saved.agents.filter(agent => !HARNESS_SLUGS.includes(agent)) : [])];
  const on = (agent: string) => allowed.kind === "any" || allowed.agents.includes(agent);
  const toggle = (agent: string) => {
    const picked = shown.filter(name => on(name) !== (name === agent));
    setAllowed(picked.length === shown.length ? { kind: "any" } : { kind: "only", agents: picked });
  };
  const submit = () => {
    if (busy) return;
    setBusy(true);
    setError("");
    save(allowed).catch(e => { setError(reason(e)); setBusy(false); });
  };

  return <>{head}
    <span className="dchips" role="group" aria-label="Harnesses">{shown.map(agent => <ChipToggle key={agent} label={harnessName(agent)} selected={on(agent)} onToggle={() => toggle(agent)} />)}</span>
    <div className="am-sub">{allowed.kind === "any" ? "Every harness may run in this project." : allowed.agents.length ? "Sessions of other harnesses do not start in this project, on any machine." : "No session starts in this project."}</div>
    <div className="am-msg bad" role="status" hidden={!error}>{error}</div>
    <div className="am-line ip-acts"><Button look="quiet" onClick={cancel}>Cancel</Button><Button look="primary" disabled={busy} onClick={submit}>{busy ? "Saving…" : "Save"}</Button></div>
  </>;
}
