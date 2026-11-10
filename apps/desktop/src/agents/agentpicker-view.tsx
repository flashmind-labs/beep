import { Spinner } from "../ui/kit.tsx";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { agentKind } from "./agents.ts";
import { effortNote, effortsOf, placePopover, railOf, type ModelRow } from "./agentpick.ts";
import { HarnessIcon } from "../ui/icons.tsx";
import type { Picked } from "./agentpicker.ts";
import { useDialogRef } from "../ui/parts.ts";

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
  /// The pill the picker opens from, and the area it stays inside.
  anchor: () => HTMLElement | null;
  bounds: () => HTMLElement | null;
  search: RefObject<HTMLInputElement | null>;
}

type Placement = ReturnType<typeof placePopover>;

/// Puts the picker above its pill, right edges lined up, whole and inside its
/// bounds: below the pill when it does not fit above, else cut to the roomier
/// side, where its columns scroll. It is measured unplaced first, then placed,
/// both before the frame paints.
function usePlacement(content: string, anchor: () => HTMLElement | null, bounds: () => HTMLElement | null) {
  const node = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<Placement | null>(null);
  const where = useRef({ anchor, bounds });
  where.current = { anchor, bounds };

  useLayoutEffect(() => { setPlace(null); }, [content]);
  useLayoutEffect(() => {
    const pick = node.current, pill = where.current.anchor(), area = where.current.bounds();
    if (place || !pick || !pill || !area) return;
    const pane = area.getBoundingClientRect(), a = pill.getBoundingClientRect();
    const top = Math.max(0, pane.top), left = Math.max(0, pane.left);
    const view = { width: Math.min(pane.right, window.innerWidth) - left, height: Math.min(pane.bottom, window.innerHeight) - top };
    const at = placePopover({ top: a.top - top, left: a.left - left, width: a.width, height: a.height }, { width: pick.offsetWidth, height: pick.offsetHeight }, view);
    setPlace({ ...at, top: at.top + top, left: at.left + left });
  }, [place]);
  useEffect(() => {
    const again = () => setPlace(null);
    window.addEventListener("resize", again);
    window.addEventListener("scroll", again, { capture: true, passive: true });
    return () => {
      window.removeEventListener("resize", again);
      window.removeEventListener("scroll", again, { capture: true });
    };
  }, []);

  const style: CSSProperties | undefined = place ? { top: `${place.top}px`, left: `${place.left}px`, width: `${place.width}px`, ...(place.maxHeight === null ? {} : { "--pick-h": `${place.maxHeight}px` }) } : undefined;
  return { node, style };
}

function Check() {
  return <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-10"/></svg>;
}

export function AgentPillContent({ agent, detail }: { agent: string; detail: string }) {
  return <><HarnessIcon agent={agent}/>{detail}<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6" /></svg></>;
}

export function AgentPicker({ picked, offered, everywhere, machineName, machineId, rows, note, loading, failed, onRetry, onQuery, onAgent, onModel, onEffort, anchor, bounds, search }: PickerProps) {
  const name = agentKind(picked.agent).name, levels = effortsOf(picked.agent);
  // The highlighted row is kept by its model, so a list that streams in
  // keeps it where it still shows; another harness or machine starts over.
  const [lit, setLit] = useState<string | null>(null);
  const highlight = lit === null ? -1 : rows.findIndex(row => row.value === lit);
  useLayoutEffect(() => { setLit(null); }, [machineId, picked.agent]);
  const litRow = useRef<HTMLButtonElement>(null);
  const move = (to: number) => { const row = rows[to]; if (!row) return; setLit(row.value); };
  useLayoutEffect(() => { litRow.current?.scrollIntoView({ block: "nearest" }); }, [lit]);
  const resultKey = JSON.stringify([machineId, picked.agent, rows]);
  const { node, style } = usePlacement(JSON.stringify([resultKey, note, loading, failed, levels.length]), anchor, bounds);
  const dialog = useDialogRef(node);
  return <div className="agc-pick" role="dialog" aria-label="Harness, model and thinking" ref={dialog} style={style}>
    <div className="agc-rail">{railOf(offered, everywhere).map(item => {
      const kind = agentKind(item.agent);
      const title = item.enabled ? kind.name : `${kind.name} (not on ${machineName || "any machine"})`;
      return <button key={item.agent} type="button" data-agh={item.agent} aria-pressed={item.agent === picked.agent}
        title={title} disabled={!item.enabled} onClick={() => { setLit(null); onAgent(item.agent); }}><HarnessIcon agent={item.agent}/></button>;
    })}</div>
    <div className="agc-list"><div className="agc-find"><input ref={search} className="agc-search" spellCheck={false} autoComplete="off"
      placeholder={`Search ${name} models…`} value={picked.query} onChange={event => { setLit(null); onQuery(event.currentTarget.value); }}
      onKeyDown={event => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          move(event.key === "ArrowDown" ? Math.min(highlight + 1, rows.length - 1) : Math.max(highlight - 1, 0));
        } else if (event.key === "Enter" && !event.metaKey) {
          event.preventDefault();
          const row = rows[highlight] ?? rows[0];
          if (row) onModel(row.value);
        }
      }}/></div>
      <div className="agc-models">{rows.map((row, index) => <button key={row.value} ref={index === highlight ? litRow : null} type="button" data-agmo={row.value}
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
