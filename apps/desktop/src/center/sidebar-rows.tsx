import { Icon, PROJECT_COLORS } from "../ui/icons.tsx";
import type { Placed, Project } from "../project/projects-state.ts";
import type { ProjectWindowProps } from "../project/projectwindow.ts";

function Pin({ id, pinned, toggle }: { id: string; pinned: boolean; toggle(): void }) {
  return <button type="button" className="pa" data-id={id} aria-label={pinned ? "Unpin" : "Pin to top"} onClick={event => { event.stopPropagation(); toggle(); }}><Icon name="pin" /></button>;
}
export function SidebarProjectRow({ project, label, pinned, indented, current, open, pin, windowProps }: { project: Placed<Project>; label: string; pinned: boolean; indented: boolean; current: boolean; open(): void; pin(): void; windowProps: ProjectWindowProps }) {
  return <div className={`cc-si cc-chat${indented ? " pj-repo" : ""}`} onClick={open} {...windowProps.menu}><button {...windowProps.clicks} type="button" className="open" aria-label={project.name} title={[project.name, ...project.machines.map(machine => `${machine.name}: ${machine.folder}`)].join("\n")} aria-current={current ? "true" : undefined}><span className="cc-pdot" style={{ background: PROJECT_COLORS[project.i % PROJECT_COLORS.length] }} /><span className="tt">{label}</span>{!!project.agents && <span className="n">{project.agents}</span>}</button><Pin id={project.name} pinned={pinned} toggle={pin} /></div>;
}
