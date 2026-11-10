import { Icon, PROJECT_COLORS } from "../ui/icons.tsx";
import type { Placed, Project } from "../project/projects-state.ts";
import type { ThreadChatEntry } from "./sidechats.ts";
import type { SideChat } from "./state.ts";
import { Spinner } from "../ui/kit.tsx";
import type { ProjectWindowProps } from "../project/projectwindow.ts";

function Pin({ id, pinned, toggle }: { id: string; pinned: boolean; toggle(): void }) {
  return <button type="button" className="pa" data-id={id} aria-label={pinned ? "Unpin" : "Pin to top"} onClick={event => { event.stopPropagation(); toggle(); }}><Icon name="pin" /></button>;
}
export function SidebarChatRow({ chat, current, answering, open, pin, menu }: { chat: Exclude<SideChat, { kind: "project" }>; current: boolean; answering: boolean; open(): void; pin(): void; menu(event: MouseEvent): void }) {
  return <div className="cc-si cc-chat" onClick={open} onContextMenu={event => menu(event.nativeEvent)}><button type="button" className="open" title={chat.title} aria-label={chat.title} aria-current={current ? "true" : undefined}><span className="sidebar-icon" style={{ display: "contents" }}>{chat.kind === "group" ? <Icon name="group" /> : chat.research ? <Icon name="doc" size={14} /> : <Icon name="chat" />}</span><span className="tt">{chat.title}</span></button>{answering ? <Spinner dot className="cc-run" label="Answering" title="Answering" /> : <Pin id={chat.id} pinned={chat.pinned} toggle={pin} />}</div>;
}
export function SidebarProjectRow({ project, label, pinned, indented, current, threads, activeChat, running, open, pin, windowProps, openThread }: { project: Placed<Project>; label: string; pinned: boolean; indented: boolean; current: boolean; threads: ThreadChatEntry[]; activeChat: string | null; running: Set<string>; open(): void; pin(): void; windowProps: ProjectWindowProps; openThread(id: string): void }) {
  const projectNode = <div className={`cc-si cc-chat${indented ? " pj-repo" : ""}`} onClick={open} {...windowProps.menu}><button {...windowProps.clicks} type="button" className="open" aria-label={project.name} title={[project.name, ...project.machines.map(machine => `${machine.name}: ${machine.folder}`)].join("\n")} aria-current={current ? "true" : undefined}><span className="cc-pdot" style={{ background: PROJECT_COLORS[project.i % PROJECT_COLORS.length] }} /><span className="tt">{label}</span>{!!project.agents && <span className="n">{project.agents}</span>}</button><Pin id={project.name} pinned={pinned} toggle={pin} /></div>;
  return <div className="cc-pjthreads">{projectNode}{threads.map(thread => { const title = thread.title || `${thread.kind === "pr" ? "PR" : "Issue"} #${thread.number}`; return <div className="cc-si cc-chat cc-thread" key={thread.chat_id}><button type="button" className="open" title={`${title} (${thread.project})`} aria-current={activeChat === thread.chat_id ? "true" : undefined} onClick={event => { event.stopPropagation(); openThread(thread.chat_id); }}><span className="tt">{title}</span></button>{running.has(thread.chat_id) && <Spinner dot className="cc-run" label="Answering" title="Answering" />}</div>; })}</div>;
}
