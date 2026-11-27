import { useSyncExternalStore, type ReactElement, type ReactNode } from "react";
import { Icon } from "../../ui/icons.tsx";
import { Button, Spinner } from "../../ui/kit.tsx";
import type { Feedback } from "./feedback.ts";

/// The agent button: Review with agent in a diff and on a pull request,
/// Work on it on an issue. `hint` is its tooltip, `reason` why it cannot start.
export type FeedbackState =
  | { kind: "running"; label: string; agent: string; hint: string }
  | { kind: "checking" }
  | { kind: "starting" }
  | { kind: "ready"; label: string; hint: string; reason: string; open: boolean };

interface FeedbackButtonProps {
  state: FeedbackState;
  setButton: (button: HTMLButtonElement | null) => void;
  setSlot: (slot: HTMLDivElement | null) => void;
  onClick: () => void;
  /// The harness and model picker, open under the button.
  picker: ReactNode;
}

/// Review with agent in the diff's header.
export function FeedbackBox({ feedback }: { feedback: Feedback }) {
  const { state, picker } = useSyncExternalStore(feedback.subscribe, feedback.snapshot);
  return <span className="dv-fbw" ref={feedback.setBox}>
    <FeedbackControl state={state} setButton={feedback.setButton} setSlot={feedback.setSlot} onClick={feedback.click} picker={picker} />
  </span>;
}

/// Why the last review failed, under the header.
export function FeedbackNoteBox({ feedback }: { feedback: Feedback }) {
  const { note } = useSyncExternalStore(feedback.subscribe, feedback.snapshot);
  return <div className="dv-fbnote" hidden={!note} role={note ? "status" : undefined}>
    {note && <FeedbackNote text={note} onDismiss={feedback.dismiss} />}
  </div>;
}

export function FeedbackControl({ state, setButton, setSlot, onClick, picker }: FeedbackButtonProps) {
  const disabled = state.kind === "checking" || state.kind === "starting" || (state.kind === "ready" && !!state.reason);
  const title = state.kind === "running" ? state.hint : state.kind === "ready" ? state.reason || state.hint : "";
  const expanded = state.kind === "ready" && !state.reason ? state.open : undefined;
  return <>
    <button ref={setButton} type="button" className="dv-btn dv-fb" disabled={disabled} title={title} aria-expanded={expanded}
      onClick={event => { event.stopPropagation(); onClick(); }}>
      {state.kind === "running" ? <><Spinner />{state.label} <small>{state.agent}</small></>
        : state.kind === "checking" ? <><Spinner />Finding session</>
        : state.kind === "starting" ? <><Spinner />Starting</>
        : <>{state.label}<span style={{ display: "grid" }}><Icon name="caretDown" size={12} /></span></>}
    </button>
    <div ref={setSlot}>{picker}</div>
  </>;
}

export function FeedbackNote({ text, onDismiss }: { text: string; onDismiss: () => void }) {
  return <><b>The review did not finish</b><pre>{text}</pre><div className="a">
    <button type="button" className="dv-btn" onClick={onDismiss}>Dismiss</button>
  </div></>;
}

/// The agent button as its owner has it; the picker adds whether it is open.
/// `running` and `done` open the session it started.
export type AgentState =
  | { kind: "ready"; label: string; hint: string }
  | { kind: "checking" }
  | { kind: "starting" }
  | { kind: "running"; label: string; agent: string; hint: string; open(): void }
  | { kind: "done"; label: string; hint: string; open(): void };

/// The agent button's picker, kept per button across draws.
export interface PickButton {
  snapshot: () => { open: boolean; picker: ReactElement | null };
  subscribe: (listener: () => void) => () => void;
  setBox: (box: HTMLElement | null) => void;
  setButton: (button: HTMLButtonElement | null) => void;
  setSlot: (slot: HTMLDivElement | null) => void;
  click: () => void;
  /// Why it cannot start, or "".
  why: () => string;
}

/// An agent button as its owner draws it, and why its last start failed.
export interface AgentButtonItem { button: PickButton; state: AgentState; error: string }

/// The agent button outside a diff, and why its last start failed: its
/// picker's pick starts a session.
export function AgentButton({ item }: { item: AgentButtonItem }) {
  const { state } = item;
  return <>{state.kind === "done" ? <Button title={state.hint} onClick={state.open}>{state.label}</Button> : <PickControl button={item.button} state={state} />}
    {item.error && <span className="cc-err">{item.error}</span>}</>;
}

function PickControl({ button, state }: { button: PickButton; state: Exclude<AgentState, { kind: "done" }> }) {
  const { open, picker } = useSyncExternalStore(button.subscribe, button.snapshot);
  const shown: FeedbackState = state.kind === "ready" ? { ...state, reason: button.why(), open }
    : state.kind === "running" ? { kind: "running", label: state.label, agent: state.agent, hint: state.hint } : state;
  return <span className="dv-fbw" ref={button.setBox}>
    <FeedbackControl state={shown} setButton={button.setButton} setSlot={button.setSlot} onClick={state.kind === "running" ? state.open : button.click} picker={picker} />
  </span>;
}
