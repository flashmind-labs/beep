import { memo } from "react";
import { conflictCount } from "../../parts/project/filesync.ts";
import { projectName } from "../../stores/project/projects-state.ts";
import { spoken } from "../../ui/format.ts";
import { Icon, PROJECT_COLORS } from "../../ui/icons.tsx";
import type { Placed, Project } from "../../stores/project/projects-state.ts";
import type { ProjectWindowProps } from "../../stores/project/projectwindow.ts";

function Pin({ id, pinned, toggle }: { id: string; pinned: boolean; toggle(): void }) {
  return <button type="button" className="pa" data-id={id} aria-label={pinned ? "Unpin" : "Pin to top"} onClick={event => { event.stopPropagation(); toggle(); }}><Icon name="pin" /></button>;
}

const sameMachines = (a: Project["machines"], b: Project["machines"]) =>
  a === b || (a.length === b.length && a.every((m, i) => m.name === b[i].name && m.folder === b[i].folder));

/// A project in the sidebar: its running agents, and its sync conflicts as a hot count.
export const SidebarProjectRow = memo(function SidebarProjectRow({ project, label, pinned, indented, conflicts, current, open, pin, windowProps }: { project: Placed<Project>; label: string; pinned: boolean; indented: boolean; conflicts: number; current: boolean; open(): void; pin(): void; windowProps: ProjectWindowProps }) {
  return <div className={`cc-si cc-chat${indented ? " pj-repo" : ""}`} onClick={open} {...windowProps.menu}><button {...windowProps.clicks} type="button" className="open" aria-label={spoken(projectName(project), project.agents, conflicts)} title={[projectName(project), ...project.machines.map(machine => `${machine.name}: ${machine.folder}`)].join("\n")} aria-current={current ? "true" : undefined}><span className="cc-pdot" style={{ background: PROJECT_COLORS[project.i % PROJECT_COLORS.length] }} /><span className="tt">{label}</span>{!!project.agents && <>{" "}<span className="n">{project.agents}</span></>}{!!conflicts && <>{" "}<span className="n hot" title={conflictCount(conflicts)} style={{ marginLeft: project.agents ? 0 : undefined }}>{conflicts}</span></>}</button><Pin id={project.name} pinned={pinned} toggle={pin} /></div>;
}, (prev, next) => prev.project.name === next.project.name && prev.project.agents === next.project.agents && prev.project.i === next.project.i
  && prev.label === next.label && prev.pinned === next.pinned && prev.indented === next.indented && prev.conflicts === next.conflicts
  && prev.current === next.current && sameMachines(prev.project.machines, next.project.machines));
