import { Button } from "../ui/kit.tsx";

export function HistoryNavigation({ query, back, refresh, collapsed, toggleSections, search }: {
  query: string;
  back: () => void;
  refresh: () => void;
  collapsed: boolean;
  toggleSections: () => void;
  search: (query: string) => void;
}) {
  return <><div className="gh-nav-head">
    <Button look="plain" className="gh-back" onClick={back}>Project</Button>
    <b>Git</b>
    <Button look="plain" className="gh-collapse" onClick={refresh}>Refresh</Button>
    <Button look="plain" className="gh-collapse" onClick={toggleSections}>{collapsed ? "Expand all" : "Collapse all"}</Button>
  </div>
    <label className="gh-search"><input type="search" maxLength={256} placeholder="Search commit messages" aria-label="Search commit messages" defaultValue={query} onChange={event => search(event.currentTarget.value)} /></label></>;
}
