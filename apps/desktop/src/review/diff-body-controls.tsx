import { type DiffGap, type Extent } from "./diff-state.ts";

type Direction = keyof Extent;
interface ContextProps { direction: Direction; expand(): void }

const ARROW = <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12h14M13 6l6 6-6 6" /></svg>;

/// The bar over a file's first hunk and under its last.
export function DiffContext({ direction, expand }: ContextProps) {
  const label = `Show 10 lines ${direction}`;
  return <button type="button" className={`dv-btn dv-context ${direction}`} title={label} aria-label={label} onClick={event => { event.stopPropagation(); expand(); }}>
    <span className="dv-context-g"><span style={{ display: "grid" }}>{ARROW}</span></span>
  </button>;
}

/// The arrows in a hunk header's gutter that show the lines left out before
/// it: the next ten after the hunk above, the last ten before this one, or
/// all of a short gap.
export function DiffGapButtons({ gap, expand }: { gap: DiffGap; expand: (direction: Direction, after: number) => void }) {
  const button = (direction: Direction, label: string, wide: boolean) =>
    <button type="button" className={`dv-gap-btn ${direction}${wide ? " wide" : ""}`} title={label} aria-label={label} onClick={() => expand(direction, gap.after)}>{ARROW}</button>;

  if (gap.lines <= 10) return button("below", `Show ${gap.lines} ${gap.lines === 1 ? "line" : "lines"}`, true);
  return <>{button("below", "Show the next 10 lines", false)}{button("above", "Show the previous 10 lines", false)}</>;
}

export function DiffOmitted({ text, label, load, busy }: { text: string; label: string | null; load: () => void; busy: boolean }) {
  return <div className="dv-note">{text}{label && <button type="button" className="dv-btn" disabled={busy} onClick={event => { event.stopPropagation(); load(); }}>{busy ? "Loading…" : label}</button>}</div>;
}
