export type FeedbackState =
  | { kind: "running"; agent: string }
  | { kind: "starting" }
  | { kind: "ready"; reason: string; open: boolean };

interface FeedbackButtonProps {
  state: FeedbackState;
  setButton: (button: HTMLButtonElement | null) => void;
  setSlot: (slot: HTMLDivElement | null) => void;
  onClick: () => void;
}

export function FeedbackControl({ state, setButton, setSlot, onClick }: FeedbackButtonProps) {
  const disabled = state.kind === "starting" || (state.kind === "ready" && !!state.reason);
  const title = state.kind === "running" ? "Open the review. The reviewer only reads; its findings become comments here"
    : state.kind === "starting" ? "" : state.reason || "Ask an agent to review these changes. It only reads. Pick a model to start";
  const expanded = state.kind === "ready" && !state.reason ? state.open : undefined;
  return <>
    <button ref={setButton} type="button" className="dv-btn dv-fb" disabled={disabled} title={title} aria-expanded={expanded}
      onClick={event => { event.stopPropagation(); onClick(); }}>
      {state.kind === "running" ? <><span className="dspin"/>Reviewing <small>{state.agent}</small></>
        : state.kind === "starting" ? <><span className="dspin"/>Starting</>
        : <>Review with agent<span style={{ display: "grid" }}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6"/></svg></span></>}
    </button>
    <div ref={setSlot}/>
  </>;
}

export function FeedbackNote({ text, onDismiss }: { text: string; onDismiss: () => void }) {
  return <><b>The review did not finish</b><pre>{text}</pre><div className="a">
    <button type="button" className="dv-btn" onClick={onDismiss}>Dismiss</button>
  </div></>;
}
