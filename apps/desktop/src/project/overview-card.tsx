import { memo, useLayoutEffect, useRef } from "react";
import { shallow } from "zustand/shallow";
import { formatAgo, plural } from "../ui/format.ts";
import { IC, PROJECT_COLORS } from "../ui/icons.tsx";
import { barSegments, countsLine, latestLine, workersLabel, type OverviewCard, type OverviewCardRow, type Project } from "./projects-state.ts";

export function overviewCardModel(c: OverviewCardRow<Project, OverviewCard>, sync: string | null) {
  const [open = "", ...rest] = c.sum ? countsLine(c.sum.tasks) : [];
  const tasks = c.sum?.tasks;
  return {
    name: c.name, repo: c.repo, index: c.i, pinned: c.pinned, sync,
    workers: c.sum ? workersLabel(c.sum.workers) : c.agents ? plural(c.agents, "agent", "agents") : "idle",
    hasSummary: !!c.sum, open, rest: rest.map(text => ` · ${text}`).join(""),
    running: tasks?.running ?? 0, review: tasks?.review ?? 0, blocked: tasks?.blocked ?? 0, planned: tasks?.planned ?? 0, done: tasks?.done ?? 0,
    latest: c.sum ? latestLine(c.sum.latest) : c.last ? `Last activity ${formatAgo(c.last, "since")}` : "No recent activity",
    latestAge: c.sum?.latest ? formatAgo(c.sum.latest.updated_at, "short") : "",
    hasMachines: !!c.machines.length, machines: c.machines.map(machine => machine.name).join(", "),
  };
}

type CardModel = ReturnType<typeof overviewCardModel>;
export const OverviewCardView = memo(function OverviewCardView({ model: c, open, pin, wire }: { model: CardModel; open(name: string): void; pin(name: string, pinned: boolean): void; wire(clicks: HTMLElement, menu: HTMLElement, name: string): void }) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (ref.current) wire(ref.current, ref.current, c.name); }, [c.name, wire]);
  const segments = c.hasSummary ? barSegments({ running: c.running, review: c.review, blocked: c.blocked, planned: c.planned, done: c.done }) : [];
  return <div ref={ref} className="pj-card" role="button" tabIndex={0} title={c.name} onClick={() => open(c.name)} onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); open(c.name); } }}><div className="h"><span className="cc-pdot" style={{ background: PROJECT_COLORS[c.index % PROJECT_COLORS.length] }} /><b title={c.name}>{c.repo}</b><button type="button" className={`pa${c.pinned ? " on" : ""}`} aria-label={c.pinned ? "Unpin" : "Pin to top"} onClick={e => { e.stopPropagation(); pin(c.name, !c.pinned); }} dangerouslySetInnerHTML={{ __html: IC.pin }} /><small>{c.workers}</small></div>
    {c.hasSummary ? <><div className="pj-bar">{segments.map(segment => <i key={segment.state} className={segment.state} style={{ width: `${segment.share}%` }} title={`${segment.n} ${segment.state}`} />)}</div><span className="line"><b>{c.open}</b>{c.rest}</span><span className="line">{c.latest}{c.latestAge && <small> · {c.latestAge}</small>}</span></> : <span className="line">{c.latest}</span>}
    {c.hasMachines && <span className="line sync">on {c.machines}</span>}{c.sync && <span className="line sync">{c.sync}</span>}</div>;
}, (before, after) => shallow(before.model, after.model) && before.open === after.open && before.pin === after.pin && before.wire === after.wire);
