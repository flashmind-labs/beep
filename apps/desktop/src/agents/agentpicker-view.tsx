import { Spinner } from "../ui/kit.tsx";
import { useLayoutEffect, useRef, useState } from "react";
import { agentKind } from "./agents.ts";
import { effortNote, effortsOf, railOf, type ModelRow } from "./agentpick.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import type { Picked } from "./agentpicker.ts";

interface PickerProps {
  picked: Picked;
  offered: readonly string[];
  everywhere: readonly string[];
  machineName: string | null;
  machineId: string | null;
  rows: readonly ModelRow[];
  note: string;
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
  onQuery: (query: string) => void;
  onAgent: (agent: string) => void;
  onModel: (model: string) => void;
  onEffort: (effort: string | null) => void;
}

function Check() {
  return <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-10"/></svg>;
}

export function AgentPillContent({ agent, detail }: { agent: string; detail: string }) {
  return <><HarnessIcon agent={agent}/>{detail}<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6" /></svg></>;
}

export function AgentPicker({ picked, offered, everywhere, machineName, machineId, rows, note, loading, failed, onRetry, onQuery, onAgent, onModel, onEffort }: PickerProps) {
  const name = agentKind(picked.agent).name, levels = effortsOf(picked.agent);
  const [highlight, setHighlight] = useState(-1);
  const models = useRef<HTMLDivElement>(null);
  const resultKey = JSON.stringify([machineId, picked.agent, rows]);
  useLayoutEffect(() => { setHighlight(-1); }, [resultKey]);
  useLayoutEffect(() => { if (highlight >= 0) models.current?.querySelector<HTMLElement>(".hi")?.scrollIntoView({ block: "nearest" }); }, [highlight]);
  return <div className="agc-pick" role="dialog" aria-label="Harness, model and thinking">
    <div className="agc-rail">{railOf(offered, everywhere).map(item => {
      const kind = agentKind(item.agent);
      const title = item.enabled ? kind.name : `${kind.name} (not on ${machineName || "any machine"})`;
      return <button key={item.agent} type="button" data-agh={item.agent} aria-pressed={item.agent === picked.agent}
        title={title} disabled={!item.enabled} onClick={() => { setHighlight(-1); onAgent(item.agent); }}><HarnessIcon agent={item.agent}/></button>;
    })}</div>
    <div className="agc-list"><div className="agc-find"><input className="agc-search" spellCheck={false} autoComplete="off"
      placeholder={`Search ${name} models…`} value={picked.query} onChange={event => { setHighlight(-1); onQuery(event.currentTarget.value); }}
      onKeyDown={event => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          if (rows.length) setHighlight(event.key === "ArrowDown" ? Math.min(highlight + 1, rows.length - 1) : Math.max(highlight - 1, 0));
        } else if (event.key === "Enter" && !event.metaKey) {
          event.preventDefault();
          const row = rows[highlight] ?? rows[0];
          if (row) onModel(row.value);
        }
      }}/></div>
      <div className="agc-models" ref={models}>{rows.map((row, index) => <button key={row.value} type="button" data-agmo={row.value}
        className={`${row.value === picked.model ? "on" : ""}${row.value && picked.agent === "flsh" ? " mono" : ""}${index === highlight ? " hi" : ""}`}
        onClick={() => onModel(row.value)}>{row.label}{row.note && <small>{row.note}</small>}</button>)}
        {note && <p className="none" role="status">{loading && <><Spinner /> </>}{note}</p>}
        {failed && <button type="button" data-agretry="" onClick={onRetry}>Retry</button>}
      </div>
    </div>
    <div className="agc-think"><h5><span>Thinking</span><span>{name}</span></h5>{levels.length ? <>
      <button type="button" data-age="" className={picked.effort ? "" : "on"} onClick={() => onEffort(null)}>Default{!picked.effort && <Check/>}</button>
      {levels.map(level => <button key={level.value} type="button" data-age={level.value} className={picked.effort === level.value ? "on" : ""}
        onClick={() => onEffort(level.value)}>{level.label}{picked.effort === level.value && <Check/>}</button>)}
    </> : <p className="none">{effortNote(name)}</p>}</div>
  </div>;
}
