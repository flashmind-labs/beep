import { memo } from "react";
import { shallow } from "zustand/shallow";
import { formatAgo, plural } from "../ui/format.ts";
import { Icon, PROJECT_COLORS } from "../ui/icons.tsx";
import { onRowKey } from "../ui/kit.tsx";
import type { Project } from "./projects-state.ts";
import type { ProjectWindowProps } from "./projectwindow.ts";
import type { OverviewCardRow } from "./overview-state.ts";

export function overviewCardModel(c: OverviewCardRow<Project>, sync: string | null) {
  return {
    name: c.name, repo: c.repo, index: c.i, pinned: c.pinned, sync,
    agents: c.agents ? plural(c.agents, "agent", "agents") : "idle",
    latest: c.last ? `Last activity ${formatAgo(c.last, "since")}` : "No recent activity",
    hasMachines: !!c.machines.length, machines: c.machines.map(machine => machine.name).join(", "),
  };
}

type CardModel = ReturnType<typeof overviewCardModel>;
/// `windowProps` acts on the card's name only, so a new one alone draws nothing again.
export const OverviewCardView = memo(function OverviewCardView({ model: c, open, pin, windowProps }: { model: CardModel; open(name: string): void; pin(name: string, pinned: boolean): void; windowProps: ProjectWindowProps }) {
  return <div className="pj-card" role="button" tabIndex={0} title={c.name} onClick={() => open(c.name)} onKeyDown={onRowKey(() => open(c.name))} {...windowProps.clicks} {...windowProps.menu}><div className="h"><span className="cc-pdot" style={{ background: PROJECT_COLORS[c.index % PROJECT_COLORS.length] }} /><b title={c.name}>{c.repo}</b><button type="button" className={`pa${c.pinned ? " on" : ""}`} aria-label={c.pinned ? "Unpin" : "Pin to top"} onClick={e => { e.stopPropagation(); pin(c.name, !c.pinned); }}><Icon name="pin" /></button><small>{c.agents}</small></div>
    <span className="line">{c.latest}</span>
    {c.hasMachines && <span className="line sync">on {c.machines}</span>}{c.sync && <span className="line sync">{c.sync}</span>}</div>;
}, (before, after) => shallow(before.model, after.model) && before.open === after.open && before.pin === after.pin);
