import { memo, type CSSProperties, type ReactNode } from "react";
import { shallow } from "zustand/shallow";
import { formatAgo, plural } from "../ui/format.ts";
import { Icon, PROJECT_COLORS } from "../ui/icons.tsx";
import { onRowKey } from "../ui/kit.tsx";
import type { Project } from "./projects-state.ts";
import type { ProjectWindowProps } from "./projectwindow.ts";
import type { OverviewCardRow } from "./overview-state.ts";

/// A project's card: its name, how many agents run there now, when one last did, and where it lives.
export function overviewCardModel(c: OverviewCardRow<Project>, sync: string | null) {
  const [first, ...rest] = c.machines.map(machine => machine.name);
  return {
    name: c.name, repo: c.repo, index: c.i, pinned: c.pinned, sync,
    running: c.agents ? plural(c.agents, "agent", "agents") : null,
    ago: c.last ? formatAgo(c.last, "since") : null,
    where: first ? rest.length ? `${first} +${rest.length}` : first : null, everywhere: [first, ...rest].filter(Boolean).join(", "),
  };
}

type CardModel = ReturnType<typeof overviewCardModel>;
/// `work` is the rows of the project's running sessions, which say what each does and open it;
/// null when none runs. `windowProps` acts on the card's name only, so a new one alone draws
/// nothing again; a card with sessions always draws, since their rows follow moves and closes.
export const OverviewCardView = memo(function OverviewCardView({ model: c, work, open, pin, windowProps }: { model: CardModel; work: ReactNode; open(name: string): void; pin(name: string, pinned: boolean): void; windowProps: ProjectWindowProps }) {
  return <div className="pj-card" data-live={c.running ? "" : undefined} role="button" tabIndex={0} title={c.name} onClick={() => open(c.name)} onKeyDown={onRowKey(() => open(c.name))} {...windowProps.clicks} {...windowProps.menu}><div className="h"><span className="pj-swatch" style={{ "--c": PROJECT_COLORS[c.index % PROJECT_COLORS.length] } as CSSProperties} /><b title={c.name}>{c.repo}</b><button type="button" className={`pa${c.pinned ? " on" : ""}`} aria-label={c.pinned ? "Unpin" : "Pin to top"} onClick={e => { e.stopPropagation(); pin(c.name, !c.pinned); }}><Icon name="pin" /></button>{c.running && <small className="run"><i />{c.running}</small>}</div>
    {work && <div className="pj-work" onClick={e => e.stopPropagation()} onContextMenu={e => e.stopPropagation()}>{work}</div>}
    <div className="f"><span className={c.ago ? "ago" : "ago never"}>{c.ago ?? "No activity"}</span>{c.where && <span className="at" title={c.everywhere}>{c.where}</span>}{c.sync && <span className="sy">{c.sync}</span>}</div></div>;
}, (before, after) => !before.work && !after.work && shallow(before.model, after.model) && before.open === after.open && before.pin === after.pin);
