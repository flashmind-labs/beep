export function HistoryNavigation({ query, back, refresh, collapse, search }: {
  query: string; back: () => void; refresh: () => void; collapse: () => void; search: (query: string) => void;
}) {
  return <><div className="gh-nav-head"><button type="button" className="gh-back" onClick={back}>Project</button><b>Git</b><button type="button" className="gh-collapse" onClick={refresh}>Refresh</button><button type="button" className="gh-collapse" onClick={collapse}>Collapse all</button></div>
    <label className="gh-search"><input type="search" maxLength={256} placeholder="Search commit messages" aria-label="Search commit messages" defaultValue={query} onChange={event => search(event.currentTarget.value)} /></label></>;
}
