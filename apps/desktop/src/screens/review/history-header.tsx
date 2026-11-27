import { CopyButton } from "../../ui/kit.tsx";

type HistoryTab = "changes" | "details" | "sessions";

export function HistoryHeader({ breadcrumb, title, metadata, copyValue, copyLabel, tab, onTab }: {
  breadcrumb: string; title: string; metadata: string; copyValue: string; copyLabel: string;
  tab: HistoryTab; onTab: (tab: HistoryTab) => void;
}) {
  return <><header className="gh-detail-head"><div className="gh-breadcrumb">{breadcrumb}</div>
    <div className="gh-title-row"><h3>{title}</h3>
      <CopyButton text={copyValue} label={copyLabel} look="quiet" small /></div>
    <p>{metadata}</p></header>
    <nav className="gh-tabs" aria-label="Git views">{([["changes", "Changes"], ["details", "Details"], ["sessions", "Sessions"]] as const).map(([key, label]) =>
      <button type="button" key={key} aria-current={tab === key} onClick={() => onTab(key)}>{label}</button>)}</nav></>;
}
