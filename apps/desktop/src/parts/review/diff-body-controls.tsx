import type { ContextEdit, HunkRow } from "../../stores/review/diff-state.ts";

type Edit = (edits: readonly ContextEdit[]) => void;
interface Control { icon: "above" | "below" | "collapse"; label: string; edits: readonly ContextEdit[] }

const ARROW = <path d="M5 12h14M13 6l6 6-6 6" />;
const COLLAPSE = <path d="m7 20 5-5 5 5M7 4l5 5 5-5" />;
const HIDE = "Hide the shown lines";

function Controls({ controls, edit }: { controls: readonly Control[]; edit: Edit }) {
  return <>{controls.map(({ icon, label, edits }) =>
    <button key={icon} type="button" className={`dv-gap-btn ${icon}`} title={label} aria-label={label} onClick={event => { event.stopPropagation(); edit(edits); }}>
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">{icon === "collapse" ? COLLAPSE : ARROW}</svg>
    </button>)}</>;
}

/// The bar over a file's first hunk (`hunk` 0) and under its last: ten more
/// lines that way, and back to none once some show.
export function DiffContext({ direction, hunk, expanded, edit }: { direction: "above" | "below"; hunk: number; expanded: boolean; edit: Edit }) {
  const controls: Control[] = [{ icon: direction, label: `Show 10 lines ${direction}`, edits: [{ hunk, side: direction, show: true }] }];
  if (expanded) controls.push({ icon: "collapse", label: HIDE, edits: [{ hunk, side: direction, show: false }] });
  return <div className="dv-context"><span className="dv-context-g"><Controls controls={controls} edit={edit}/></span></div>;
}

/// The gutter of a hunk's header. With lines left out before it: the next
/// ten after the hunk above and the last ten before this one, or all of a
/// short gap. Once some show: back to none.
export function DiffGapControls({ row, edit }: { row: HunkRow; edit: Edit }) {
  const below: ContextEdit = { hunk: row.index - 1, side: "below", show: true }, above: ContextEdit = { hunk: row.index, side: "above", show: true };
  const controls: Control[] = [];

  if (row.gap > 10) controls.push({ icon: "below", label: "Show the next 10 lines", edits: [below] }, { icon: "above", label: "Show the previous 10 lines", edits: [above] });
  else if (row.gap > 0) controls.push({ icon: "below", label: `Show ${row.gap} ${row.gap === 1 ? "line" : "lines"}`, edits: [below] });
  if (row.expanded) controls.push({ icon: "collapse", label: HIDE, edits: [{ ...below, show: false }, { ...above, show: false }] });

  if (!controls.length) return <span className="dv-g"><span/><span/></span>;
  return <span className="dv-g ctl"><Controls controls={controls} edit={edit}/></span>;
}

export function DiffOmitted({ text, label, load, busy }: { text: string; label: string | null; load: () => void; busy: boolean }) {
  return <div className="dv-note">{text}{label && <button type="button" className="dv-btn" disabled={busy} onClick={event => { event.stopPropagation(); load(); }}>{busy ? "Loading…" : label}</button>}</div>;
}
