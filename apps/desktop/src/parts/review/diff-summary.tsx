import { changedLabel, grouped, type DiffView } from "../../stores/review/diff-state.ts";

interface DiffSummaryProps {
  view: DiffView | null;
  loading: boolean;
  uncommitted: boolean;
}

export function DiffSummary({ view, loading, uncommitted }: DiffSummaryProps) {
  if (!view) return <b>{loading ? "Reading the changes…" : "Changes"}</b>;
  return <>
    <b>{changedLabel(view)}</b>
    {!!view.additions && <i className="dv-plus">+{grouped(view.additions)}</i>}
    {!!view.deletions && <i className="dv-minus">-{grouped(view.deletions)}</i>}
    <span className="base">vs <code>{uncommitted ? "HEAD" : view.base}</code></span>
  </>;
}
