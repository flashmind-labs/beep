import { changedLabel, type DiffView } from "./diff-state.ts";

interface DiffSummaryProps {
  view: DiffView | null;
  loading: boolean;
  uncommitted: boolean;
}

export function DiffSummary({ view, loading, uncommitted }: DiffSummaryProps) {
  if (!view) return <b>{loading ? "Reading the changes…" : "Changes"}</b>;
  return <>
    <b>{changedLabel(view)}</b>
    {!!view.additions && <i className="dv-plus">+{view.additions.toLocaleString("en-US")}</i>}
    {!!view.deletions && <i className="dv-minus">-{view.deletions.toLocaleString("en-US")}</i>}
    <span className="base">vs <code>{uncommitted ? "HEAD" : view.base}</code></span>
  </>;
}
