import { Fragment } from "react";
import { agentOf, host, type Entry, type Research } from "./research-state.ts";
import { Avatar } from "./avatar-view.tsx";

export function ResearchSources({ research, entries }: { research: Research | null; entries: readonly Entry[] }) {
  const sources = research?.sources ?? [];
  if (!sources.length) return <div className="rpane"><div className="rbox rempty">No sources yet.</div></div>;
  const team = { research, entries };
  return <div className="rpane"><div className="rtbl" role="table">
    <span className="th">#</span><span className="th">Source</span><span className="th">Kept by</span>
    {sources.map(source => {
      return <Fragment key={source.number}>
        <span className="n">{source.number}</span>
        <span className="src"><a href={source.url}>{source.title}</a><small>{host(source.url)}</small><q>{source.quote}</q></span>
        <span className="by"><Avatar id={source.agent}/>{agentOf(team, source.agent).name}</span>
      </Fragment>;
    })}
  </div></div>;
}
