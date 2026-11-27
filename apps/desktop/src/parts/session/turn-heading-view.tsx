// Parts a turn's rows share in a coding agent's log: the heading, a step's
// mark and a running clock.
import { memo, useCallback, useLayoutEffect, useRef, useSyncExternalStore } from "react";
import { clock, duration, heading, type TurnState } from "./agentturns.ts";
import { Icon } from "../../ui/icons.tsx";
import { Spinner } from "../../ui/kit.tsx";
import { onShownChange, paneShown } from "../../ui/active.ts";

export interface TurnHeading {
  kind: "heading";
  id: string;
  state: TurnState;
  start: number | null;
  ms: number | null;
  stats: string;
  turn: string | number | null;
  opened: boolean;
}

/// A turn's heading: how long it worked and what it did. With a `turn` it is
/// a button that opens or folds the steps. A running turn's clock is on its
/// live line, so without a `turn` it has none. A finished turn of unknown
/// length has nothing to say, so it has none either.
export function turnHeading(value: Omit<TurnHeading, "kind" | "id">): TurnHeading | null {
  if (value.turn === null && (value.state !== "done" || value.ms === null)) return null;
  return { kind: "heading", id: value.turn === null ? "turn" : `steps:${value.turn}`, ...value };
}

export const AgentTurnHeading = memo(function AgentTurnHeading({ value }: { value: TurnHeading }) {
  const done = value.state === "done";
  const label = done ? <><span>{heading(value.state, value.ms)}</span>{value.stats && <span className="stats">· {value.stats}</span>}</> : <span>{value.stats || "Steps"}</span>;
  return <div className="dcoderturn">{value.turn === null ? <span className="hd">{label}</span> : <button className={`hd${value.opened ? " open" : ""}`} data-turn={value.turn} aria-expanded={value.opened} title={value.opened ? "Fold the steps" : "Show the steps"}>{label}<Icon name="chev" /></button>}{value.start !== null && <span className="when">{clock(value.start)}</span>}</div>;
});

/// How a step stands: done, failed, running, waiting on the user or did not finish.
export type StepMark = "done" | "failed" | "running" | "waiting" | "stopped" | "none";

export function stepMark(status: string, running: boolean): StepMark {
  return status === "completed" ? "done" : status === "failed" ? "failed" : status && running ? "running" : status ? "stopped" : "none";
}

export function StepMarkView({ mark }: { mark: StepMark }) {
  if (mark === "done") return <span className="dok" title="Done"><Icon name="check" /></span>;
  if (mark === "failed") return <span className="dbad" title="Failed"><Icon name="x" /></span>;
  if (mark === "running") return <Spinner dot title="Running" />;
  if (mark === "waiting") return <span className="ddot dial" title="Waiting for you" />;
  if (mark === "stopped") return <span className="ddot off" title="Did not finish" />;
  return null;
}

/// How long since `start`, moving on each second while the pane shows.
/// A clock out of the page, in a log kept for later, keeps its second and does not draw.
export function Since({ start, className, title }: { start: number; className?: string; title?: string }) {
  const el = useRef<HTMLSpanElement | null>(null), seen = useRef(second);
  const shown = () => { if (el.current?.isConnected !== false) seen.current = second; return seen.current; };
  useSyncExternalStore(subscribeSecond, shown, shown);
  useLayoutEffect(() => { tickClocks(); }, []);
  const attach = useCallback((node: HTMLSpanElement | null) => { el.current = node; return placeClock(node); }, []);
  return <span ref={attach} className={className} data-since={start} title={title}>{duration(Date.now() - start)}</span>;
}

// One timer serves every clock, and stops while none is in the page or the
// pane hides. A log put back in the page starts it with its next draw.
const clocks = new Set<() => void>();
let second = 0;
let ticking: ReturnType<typeof setInterval> | null = null;

function subscribeSecond(listener: () => void) {
  clocks.add(listener);
  tickClocks();
  return () => {
    clocks.delete(listener);
    tickClocks();
  };
}

/// The clocks' elements: a log kept out of the page keeps its clocks mounted, not shown.
const placed = new Set<HTMLElement>();
function placeClock(el: HTMLElement | null) {
  if (!el) return;
  placed.add(el);
  return () => { placed.delete(el); };
}

const shownClock = () => clocks.size > 0 && paneShown() && [...placed].some(el => el.isConnected);

/// Starts the clocks' timer when one shows. Clocks stopped while away, as in a kept log put
/// back, show the time at once.
export function tickClocks() {
  const run = shownClock();
  if (run && ticking === null) {
    ticking = setInterval(() => {
      if (!shownClock()) { tickClocks(); return; }
      nextSecond();
    }, 1000);
    nextSecond();
  } else if (!run && ticking !== null) {
    clearInterval(ticking);
    ticking = null;
  }
}

function nextSecond() {
  second++;
  for (const listener of clocks) listener();
}

onShownChange(shown => {
  const started = ticking === null;
  tickClocks();
  // What was hidden catches up at once; a timer started now did so already.
  if (shown && !(started && ticking !== null)) nextSecond();
}, true);

/// A recorded send time, never an estimate for undated history.
export function EntryTime({ at }: { at: number | null }) {
  if (at === null || !Number.isFinite(at) || at <= 0) return null;
  const date = new Date(at * 1000);
  return <time className="entry-time" dateTime={date.toISOString()} title={date.toLocaleString()}>{date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })}</time>;
}
