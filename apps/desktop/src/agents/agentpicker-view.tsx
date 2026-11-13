import { Spinner } from "../ui/kit.tsx";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
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
  const move = (to: number) => { const row = rows[to]; if (!row) return; setLit(row.value); };
  // The rows have one height, so only their number changes the picker's
  // size, and only up to the most its unplaced height shows: past that,
  // typing a search places it again for nothing.
  const sized = Math.min(rows.length, Math.ceil(PICK_HEIGHT / MODEL_ROW));
  const { node, style } = usePlacement(JSON.stringify([machineId, picked.agent, sized, note, loading, failed, levels.length]), anchor, bounds);
  const dialog = useDialogRef(node);
  const list = useRef<HTMLDivElement>(null);
  return <div className="agc-pick" role="dialog" aria-label="Harness, model and thinking" ref={dialog} style={style}>
    <div className="agc-rail">{railOf(offered, everywhere).map(item => {
      const kind = agentKind(item.agent);
      const title = item.enabled ? kind.name : `${kind.name} (not on ${machineName || "any machine"})`;
      return <button key={item.agent} type="button" data-agh={item.agent} aria-pressed={item.agent === picked.agent}
        title={title} disabled={!item.enabled} onClick={() => { setLit(null); onAgent(item.agent); }}><HarnessIcon agent={item.agent}/></button>;
    })}</div>
    <div className="agc-list" ref={list}><div className="agc-find"><input ref={search} className="agc-search" spellCheck={false} autoComplete="off"
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
      <div className="agc-models"><ModelRows rows={rows} picked={picked} highlight={highlight} lit={lit} list={list} onModel={onModel} />
        {note && <p className="none" role="status">{loading && <><Spinner /> </>}{note}</p>}
        {failed && <button type="button" data-agretry="" onClick={onRetry}>Retry</button>}
      </div>
    </div>
    <div className="agc-think">{levels.length ? <>
      <h5><span>Reasoning</span><b>{levels.find(level => level.value === picked.effort)?.label ?? "Default"}</b></h5>
      <div className="agc-track" role="radiogroup" aria-label={`Reasoning for ${name}`}>
        <button type="button" role="radio" aria-checked={!picked.effort} title="Default" data-age="" className={picked.effort ? "" : "on"} onClick={() => onEffort(null)}><span>Default</span></button>
        {levels.map(level => <button key={level.value} type="button" role="radio" aria-checked={picked.effort === level.value} title={level.label} data-age={level.value}
          className={picked.effort === level.value ? "on" : ""} onClick={() => onEffort(level.value)}><span>{level.label}</span></button>)}
      </div>
    </> : <p className="none">{effortNote(name)}</p>}</div>
  </div>;
}

/// A model row's height and the picker's tallest before it is placed, as the
/// stylesheet sets them.
const MODEL_ROW = 34;
const PICK_HEIGHT = 420;

/// The model column's rows, drawing only those near the view: a harness can
/// offer hundreds of models. The highlighted row scrolls into view as the
/// arrow keys move it, below the search field that sticks on top.
function ModelRows({ rows, picked, highlight, lit, list, onModel }: {
  rows: readonly ModelRow[];
  picked: Picked;
  highlight: number;
  lit: string | null;
  list: RefObject<HTMLDivElement | null>;
  onModel: (model: string) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  // How far down the list the rows start, under the search field.
  const [above, setAbove] = useState(0);
  useLayoutEffect(() => { setAbove((box.current?.offsetTop ?? 0) - (list.current?.offsetTop ?? 0)); }, [list]);
  const getItemKey = useCallback((index: number) => rows[index]?.value ?? index, [rows]);
  const virtual = useVirtualizer({
    count: rows.length, getScrollElement: () => list.current, estimateSize: () => MODEL_ROW, getItemKey, overscan: 8,
    scrollMargin: above, scrollPaddingStart: above, initialRect: { width: 0, height: PICK_HEIGHT },
    // A row takes the size the ResizeObserver reports before it paints;
    // measuring each as it mounts would lay the picker out once per row.
    measureElement: (node, entry, instance) => entry?.borderBoxSize?.[0]?.blockSize ?? instance.measurementsCache[instance.indexFromElement(node)]?.size ?? MODEL_ROW,
  });
  useLayoutEffect(() => { if (highlight >= 0) virtual.scrollToIndex(highlight, { align: "auto" }); }, [lit]);

  return <div ref={box} style={{ position: "relative", height: virtual.getTotalSize() }}>
    {virtual.getVirtualItems().map(item => {
      const row = rows[item.index];
      // The search field is the list's one Tab stop: a focused row would unmount as it scrolls away.
      return <button key={item.key} ref={virtual.measureElement} data-index={item.index} type="button" tabIndex={-1} data-agmo={row.value}
        className={`${row.value === picked.model ? "on" : ""}${row.value && picked.agent === "flsh" ? " mono" : ""}${item.index === highlight ? " hi" : ""}`}
        style={{ position: "absolute", top: 0, left: 0, right: 0, transform: `translateY(${item.start - above}px)` }}
        onClick={() => onModel(row.value)}>{row.label}{row.note && <small>{row.note}</small>}</button>;
    })}
  </div>;
}
