interface ContextProps { direction: "above" | "below"; expand(): void }

export function DiffContext({ direction, expand }: ContextProps) {
  const label = `Show 10 lines ${direction}`;
  return <button type="button" className={`dv-btn dv-context ${direction}`} title={label} aria-label={label} onClick={event => { event.stopPropagation(); expand(); }}>
    <span className="dv-context-g"><span style={{ display: "grid" }}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12h14M13 6l6 6-6 6" /></svg></span></span>
  </button>;
}

export function DiffOmitted({ text, label, load, busy }: { text: string; label: string | null; load: () => void; busy: boolean }) {
  return <div className="dv-note">{text}{label && <button type="button" className="dv-btn" disabled={busy} onClick={event => { event.stopPropagation(); load(); }}>{busy ? "Loading…" : label}</button>}</div>;
}
