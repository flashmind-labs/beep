import type { ModelInfo } from "./agentmodels.ts";
import { Button, NO_AUTO, Spinner, Switch } from "../ui/kit.tsx";
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type RefObject } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { agentKind } from "./agent-kind.ts";
import { type Allowed, effortNote, effortsOf, placePopover, railOf, type MachineRow, type PickRow } from "./agentpick.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import type { Picked } from "./agentpicker.tsx";
import { useDialogRef } from "../ui/parts.tsx";

interface PickerProps {
  picked: Picked;
  model: ModelInfo | null;
  offered: readonly string[];
  everywhere: readonly string[];
  allowed: Allowed;
  machineName: string | null;
  machineId: string | null;
  rows: readonly PickRow[];
  note: string;
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
  onQuery: (query: string) => void;
  onAgent: (agent: string) => void;
  onModel: (row: PickRow) => void;
  onEffort: (effort: string | null) => void;
  onFast: (fast: boolean) => void;
  /// The pill the picker opens from.
  anchor: () => HTMLElement | null;
  search: RefObject<HTMLInputElement | null>;
}

/// The stepped picker's props: the plain picker's, with the Machine step,
/// the picked model's name and the end of the last step.
interface StepPickerProps extends PickerProps {
  modelName: string;
  /// The machines the Machine step lists; null where the machine is fixed.
  machines: readonly MachineRow[] | null;
  onMachine: (id: string) => void;
  /// The last step was answered: the pick is whole.
  onDone: () => void;
}

/// The picker's steps, in the order it asks them.
type Step = "machine" | "harness" | "model" | "thinking";

const STEP_TITLES: Record<Step, string> = { machine: "Pick a machine", harness: "Pick a harness", model: "Pick a model", thinking: "Pick the thinking level" };

/// Whether `agent` asks the Thinking step after its model.
const thinks = (agent: string) => effortsOf(agent).length > 0 || agent === "codex";

type Placement = ReturnType<typeof placePopover>;

/// The highlighted row, the one Enter picks, and what put the highlight there.
interface Lit {
  key: string;
  by: "keys" | "pointer";
}

/// Puts the picker above its pill, right edges lined up, whole and inside the
/// window: below the pill when it does not fit above, else cut to the roomier
/// side, where its columns scroll. It is measured unplaced first, then placed,
/// both before the frame paints.
function usePlacement(content: string, anchor: () => HTMLElement | null) {
  const node = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<Placement | null>(null);
  const where = useRef(anchor);
  where.current = anchor;

  useLayoutEffect(() => { setPlace(null); }, [content]);
  useLayoutEffect(() => {
    const pick = node.current, pill = where.current();
    if (place || !pick || !pill) return;
    const a = pill.getBoundingClientRect();
    // The unplaced picker sits at the fixed origin. With an iPhone's keyboard up the window
    // is scrolled, and client rects are off from that origin by the scroll.
    const origin = pick.getBoundingClientRect();
    const frame = frameOf(pill);
    const at = placePopover({ top: a.top - frame.top, left: a.left - frame.left, width: a.width, height: a.height }, { width: pick.offsetWidth, height: pick.offsetHeight }, frame);
    setPlace({ ...at, top: at.top + frame.top - origin.top, left: at.left + frame.left - origin.left });
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

/// Where a picker opened from `pill` may go: the panel the pill is in, whose
/// edges cut a picker that runs past them in WebKit, else the window.
function frameOf(pill: HTMLElement) {
  const panel = pill.closest(".cc-main")?.getBoundingClientRect();
  const left = Math.max(0, panel?.left ?? 0), top = Math.max(0, panel?.top ?? 0);
  const right = Math.min(window.innerWidth, panel?.right ?? Infinity), bottom = Math.min(window.innerHeight, panel?.bottom ?? Infinity);
  return { left, top, width: right - left, height: bottom - top };
}

function Check() {
  return <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-10"/></svg>;
}

export function AgentPillContent({ agent, detail }: { agent: string; detail: string }) {
  return <><HarnessIcon agent={agent}/><span className="txt">{detail}</span><Icon name="caretDown" size={12} /></>;
}

export function AgentPicker({ picked, model, offered, everywhere, allowed, machineName, machineId, rows, note, loading, failed, onRetry, onQuery, onAgent, onModel, onEffort, onFast, anchor, search }: PickerProps) {
  const name = agentKind(picked.agent).name;
  const levels = effortsOf(picked.agent).filter(level => !model?.efforts?.length || model.efforts.includes(level.value));
  // The highlighted row is kept by its model, so a list that streams in
  // keeps it where it still shows; another harness or machine starts over.
  const [lit, setLit] = useState<Lit | null>(null);
  const highlight = lit === null ? -1 : rows.findIndex(row => rowKey(row) === lit.key);
  useLayoutEffect(() => { setLit(null); }, [machineId, picked.agent]);
  // The search takes the keys as the picker opens, so ⇧⌘M and typing picks a model.
  // A phone keeps the message field focused: moving focus re-lays the page for the
  // keyboard under the placed picker.
  useLayoutEffect(() => { if (!document.body.classList.contains("mobile")) search.current?.focus(); }, [search]);
  // The thinking level the search names, taken with the model picked.
  const effort = (rows[highlight] ?? rows[0])?.effort ?? null;
  const move = (to: number) => { const row = rows[to]; if (!row) return; setLit({ key: rowKey(row), by: "keys" }); };
  // The rows have one height, so only their number changes the picker's
  // size, and only up to the most its unplaced height shows: past that,
  // typing a search places it again for nothing.
  const sized = Math.min(rows.length, Math.ceil(PICK_HEIGHT / MODEL_ROW));
  const { node, style } = usePlacement(JSON.stringify([machineId, picked.agent, sized, note, loading, failed, levels.length]), anchor);
  const dialog = useDialogRef(node);
  const list = useRef<HTMLDivElement>(null);
  return <div className="agc-pick" role="dialog" aria-label="Harness, model and thinking" ref={dialog} style={style}>
    <div className="agc-rail">{railOf(offered, everywhere, allowed).map(item => {
      const kind = agentKind(item.agent);
      const title = item.enabled ? kind.name : `${kind.name} (not on ${machineName || "any machine"})`;
      return <button key={item.agent} type="button" data-agh={item.agent} aria-pressed={item.agent === picked.agent}
        title={title} disabled={!item.enabled} onClick={() => { setLit(null); onAgent(item.agent); }}><HarnessIcon agent={item.agent}/><span className="agc-rname">{kind.name}</span></button>;
    })}</div>
    <div className="agc-list"><div className="agc-find"><input ref={search} className="agc-search" {...NO_AUTO}
      placeholder={offered.length > 1 ? "Search models…" : `Search ${name} models…`} value={picked.query} onChange={event => { setLit(null); onQuery(event.currentTarget.value); }}
      onKeyDown={event => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          move(event.key === "ArrowDown" ? Math.min(highlight + 1, rows.length - 1) : Math.max(highlight - 1, 0));
        } else if (event.key === "Enter" && !event.metaKey) {
          event.preventDefault();
          const row = rows[highlight] ?? rows[0];
          if (row) onModel(row);
        }
      }}/></div>
      <div className="agc-models" ref={list}><ModelRows rows={rows} picked={picked} chosen highlight={highlight} lit={lit} list={list} onHover={row => setLit({ key: rowKey(row), by: "pointer" })} onModel={onModel} />
        {note && <p className="none" role="status">{loading && <><Spinner /> </>}{note}</p>}
        {failed && <button type="button" data-agretry="" onClick={onRetry}>Retry</button>}
      </div>
    </div>
    <div className="agc-think">{picked.agent === "codex" && <><h5>Fast mode<Switch label="Fast mode" on={picked.fast === true} disabled={model?.fast === false} flip={() => onFast(picked.fast !== true)} /></h5><p className="none">Higher usage.</p></>}<h5>Thinking</h5>{levels.length ? <>
      <button type="button" data-age="" className={picked.effort ? "" : "on"} onClick={() => onEffort(null)}>Default{!picked.effort && <Check/>}</button>
      {levels.map(level => <button key={level.value} type="button" data-age={level.value} className={picked.effort === level.value ? "on" : effort === level.value ? "hi" : ""}
        onClick={() => onEffort(level.value)}>{level.label}{picked.effort === level.value && <Check/>}</button>)}
    </> : <p className="none">{effortNote(name)}</p>}</div>
  </div>;
}

/// The picker under an agent button (Work on it, Continue, Review with
/// agent, Fix this) asks one step at a time, nothing picked for you: the
/// machine where it can change, the harness, the model, then the thinking
/// level, whose pick starts. A step answered shows along the top; clicking
/// it asks it again.
export function StepPicker({ picked, model, modelName, offered, everywhere, allowed, machines, machineName, machineId, rows, note, loading, failed, onRetry, onQuery, onMachine, onAgent, onModel, onEffort, onFast, onDone, anchor, search }: StepPickerProps) {
  const name = agentKind(picked.agent).name;
  const levels = effortsOf(picked.agent).filter(level => !model?.efforts?.length || model.efforts.includes(level.value));
  const steps: Step[] = [...(machines ? ["machine" as const] : []), "harness", "model", ...(thinks(picked.agent) ? ["thinking" as const] : [])];
  const [step, setStep] = useState<Step>(steps[0]);
  const at = steps.indexOf(step);
  const [lit, setLit] = useState<Lit | null>(null);
  const highlight = lit === null ? -1 : rows.findIndex(row => rowKey(row) === lit.key);
  useLayoutEffect(() => { setLit(null); }, [machineId, picked.agent]);
  const body = useRef<HTMLDivElement>(null);
  // Each step takes the keys as it shows. A phone keeps the message field
  // focused: moving focus re-lays the page for the keyboard.
  useLayoutEffect(() => {
    if (document.body.classList.contains("mobile")) return;
    if (step === "model") search.current?.focus();
    else body.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [search, step]);
  // The thinking level a search such as `opus high` names, taken with the model.
  const effort = (rows[highlight] ?? rows[0])?.effort ?? null;
  const move = (to: number) => { const row = rows[to]; if (!row) return; setLit({ key: rowKey(row), by: "keys" }); };
  const back = () => { if (at > 0) setStep(steps[at - 1]); };
  const pickModel = (row: PickRow) => {
    onModel(row);
    if (row.effort !== null || !thinks(row.agent)) onDone();
    else setStep("thinking");
  };
  const sized = Math.min(rows.length, Math.ceil(PICK_HEIGHT / MODEL_ROW));
  const { node, style } = usePlacement(JSON.stringify([step, machineId, picked.agent, sized, note, loading, failed, levels.length]), anchor);
  const dialog = useDialogRef(node);
  const list = useRef<HTMLDivElement>(null);
  const answers: Record<Step, string> = { machine: machineName ?? "", harness: name, model: modelName, thinking: "" };
  const path = steps.slice(0, at).map(s => answers[s]).join(" · ");
  return <div className="agc-pick steps" role="dialog" aria-label="Machine, harness, model and thinking" ref={dialog} style={style}>
    <div className="agc-stephead" data-agstep={step}>
      {at > 0 && <Button look="plain" className="agc-back" aria-label="Back" title="Back (⌫)" onClick={back}><Icon name="back" size={14} /></Button>}
      <div><b>{STEP_TITLES[step]}</b>{path && <small>{path}</small>}</div>
      <span>{at + 1}/{steps.length}</span>
    </div>
    <div className="agc-list" key={step} ref={body} onKeyDown={event => { if (step !== "model") stepKeys(event, back); }}>
      {step === "machine" && machines && <div className="agc-models">{machines.map(m =>
        <button key={m.id} type="button" data-agmachine={m.id} disabled={!m.enabled} onClick={() => { onMachine(m.id); setStep("harness"); }}>
          <span className={`ddot ${m.online ? "on" : "off"}`} />{m.name}{m.note && <small>{m.note}</small>}
        </button>)}
      </div>}
      {step === "harness" && <div className="agc-models">{railOf(offered, everywhere, allowed).filter(item => item.enabled || everywhere.includes(item.agent)).map(item =>
        <button key={item.agent} type="button" data-agh={item.agent} disabled={!item.enabled} onClick={() => { setLit(null); onAgent(item.agent); setStep("model"); }}>
          <HarnessIcon agent={item.agent}/>{agentKind(item.agent).name}{!item.enabled && <small>not on {machineName || "any machine"}</small>}
        </button>)}
      </div>}
      {step === "model" && <>
        <div className="agc-find"><input ref={search} className="agc-search" {...NO_AUTO}
          placeholder={`Search ${name} models…`} value={picked.query} onChange={event => { setLit(null); onQuery(event.currentTarget.value); }}
          onKeyDown={event => {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              move(event.key === "ArrowDown" ? Math.min(highlight + 1, rows.length - 1) : Math.max(highlight - 1, 0));
            } else if (event.key === "Enter" && !event.metaKey) {
              event.preventDefault();
              const row = rows[highlight] ?? rows[0];
              if (row) pickModel(row);
            } else if (event.key === "Backspace" && !picked.query) {
              event.preventDefault();
              back();
            }
          }}/></div>
        <div className="agc-models" ref={list}><ModelRows rows={rows} picked={picked} chosen={false} highlight={highlight} lit={lit} list={list} onHover={row => setLit({ key: rowKey(row), by: "pointer" })} onModel={pickModel} />
          {note && <p className="none" role="status">{loading && <><Spinner /> </>}{note}</p>}
          {failed && <button type="button" data-agretry="" onClick={onRetry}>Retry</button>}
        </div>
      </>}
      {step === "thinking" && <div className="agc-think">
        {picked.agent === "codex" && <><h5>Fast mode<Switch label="Fast mode" on={picked.fast === true} disabled={model?.fast === false} flip={() => onFast(picked.fast !== true)} /></h5><p className="none">Higher usage.</p></>}
        <button type="button" data-age="" onClick={() => { onEffort(null); onDone(); }}>Default</button>
        {levels.map(level => <button key={level.value} type="button" data-age={level.value} className={effort === level.value ? "hi" : ""}
          onClick={() => { onEffort(level.value); onDone(); }}>{level.label}</button>)}
        {!levels.length && <p className="none">{effortNote(name)}</p>}
      </div>}
    </div>
  </div>;
}

/// Up and Down move between a step's rows, Backspace goes to the step above.
function stepKeys(event: ReactKeyboardEvent<HTMLDivElement>, back: () => void) {
  if (event.key === "Backspace") { event.preventDefault(); back(); return; }
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
  const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  const at = buttons.findIndex(b => b === document.activeElement);
  event.preventDefault();
  buttons[event.key === "ArrowDown" ? Math.min(at + 1, buttons.length - 1) : Math.max(at - 1, 0)]?.focus();
}

/// A row's identity: the same model can show under several harnesses.
const rowKey = (row: PickRow) => `${row.agent}/${row.value}/${row.effort ?? ""}`;

/// A model row's height and the picker's tallest before it is placed, as the
/// stylesheet sets them.
const MODEL_ROW = 34;
const PICK_HEIGHT = 340;

/// The model column's rows, drawing only those near the view: a harness can
/// offer hundreds of models. The highlighted row scrolls into view as the
/// arrow keys move it. The cursor highlights a row only by moving over it:
/// one resting where the picker opens is not a choice.
function ModelRows({ rows, picked, chosen, highlight, lit, list, onHover, onModel }: {
  rows: readonly PickRow[];
  picked: Picked;
  /// Whether the picked model is marked: the stepped picker marks none.
  chosen: boolean;
  highlight: number;
  lit: Lit | null;
  list: RefObject<HTMLDivElement | null>;
  onHover: (row: PickRow) => void;
  onModel: (row: PickRow) => void;
}) {
  const getItemKey = useCallback((index: number) => { const row = rows[index]; return row ? rowKey(row) : index; }, [rows]);
  // Rows of another harness than the picked one carry its badge.
  const badged = rows.some(row => row.agent !== picked.agent);
  const virtual = useVirtualizer({
    count: rows.length, getScrollElement: () => list.current, estimateSize: () => MODEL_ROW, getItemKey, overscan: 8,
    initialRect: { width: 0, height: PICK_HEIGHT },
    // A row takes the size the ResizeObserver reports before it paints;
    // measuring each as it mounts would lay the picker out once per row.
    measureElement: (node, entry, instance) => entry?.borderBoxSize?.[0]?.blockSize ?? instance.measurementsCache[instance.indexFromElement(node)]?.size ?? MODEL_ROW,
  });
  useLayoutEffect(() => { if (lit?.by === "keys" && highlight >= 0) virtual.scrollToIndex(highlight, { align: "auto" }); }, [lit]);

  return <div style={{ position: "relative", height: virtual.getTotalSize() }}>
    {virtual.getVirtualItems().map(item => {
      const row = rows[item.index];
      const active = chosen && row.value === picked.model && row.agent === picked.agent;
      // The search field is the list's one Tab stop: a focused row would unmount as it scrolls away.
      return <button key={item.key} ref={virtual.measureElement} data-index={item.index} type="button" tabIndex={-1} data-agmo={row.value}
        className={`${active ? "on" : ""}${row.value && row.agent === "flsh" ? " mono" : ""}${item.index === highlight ? " hi" : ""}`}
        style={{ position: "absolute", top: 0, left: 0, right: 0, transform: `translateY(${item.start}px)` }}
        onMouseMove={event => { if (item.index !== highlight && (event.movementX || event.movementY)) onHover(row); }}
        onClick={() => onModel(row)}>{badged && <HarnessIcon agent={row.agent}/>}<span className="agc-mlbl">{row.label}</span>{row.note && <small>{row.note}</small>}{active && <span className="agc-chk" aria-hidden="true"><Check/></span>}</button>;
    })}
  </div>;
}
