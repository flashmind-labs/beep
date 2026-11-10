import { useLayoutEffect, useRef } from "react";
import { IC, PROJECT_COLORS } from "../ui/icons.ts";
import type { Placed, Project } from "../project/projects-state.ts";
import type { ThreadChatEntry } from "./sidechats.ts";
import type { SideChat } from "./state.ts";

function Icon({ svg }: { svg: string }) { return <span className="sidebar-icon" style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: svg }} />; }
function Spinner() { return <span className="drun cc-run" role="img" title="Answering" aria-label="Answering" />; }
function Pin({ id, pinned, toggle }: { id: string; pinned: boolean; toggle(): void }) {
  return <button type="button" className="pa" data-id={id} aria-label={pinned ? "Unpin" : "Pin to top"} onClick={event => { event.stopPropagation(); toggle(); }} dangerouslySetInnerHTML={{ __html: IC.pin }} />;
}
export function SidebarChatRow({ chat, current, answering, open, pin, menu }: { chat: Exclude<SideChat, { kind: "project" }>; current: boolean; answering: boolean; open(): void; pin(): void; menu(event: MouseEvent): void }) {
  return <div className="cc-si cc-chat" onClick={open} onContextMenu={event => menu(event.nativeEvent)}><button type="button" className="open" title={chat.title} aria-label={chat.title} aria-current={current ? "true" : undefined}><Icon svg={chat.kind === "group" ? IC.group : chat.research ? IC.doc : IC.chat} /><span className="tt">{chat.title}</span></button>{answering ? <Spinner /> : <Pin id={chat.id} pinned={chat.pinned} toggle={pin} />}</div>;
}
export function SidebarProjectRow({ project, label, pinned, indented, current, threads, activeChat, running, open, pin, wire, openThread }: { project: Placed<Project>; label: string; pinned: boolean; indented: boolean; current: boolean; threads: ThreadChatEntry[]; activeChat: string | null; running: Set<string>; open(): void; pin(): void; wire(button: HTMLElement, row: HTMLElement): void; openThread(id: string): void }) {
  const button = useRef<HTMLButtonElement>(null), row = useRef<HTMLDivElement>(null);
  // The project key keeps these nodes mounted; native capture handles modifier and middle clicks before React opens the project.
  useLayoutEffect(() => { if (button.current && row.current) wire(button.current, row.current); }, [project.name]);
  const projectNode = <div ref={row} className={`cc-si cc-chat${indented ? " pj-repo" : ""}`} onClick={open}><button ref={button} type="button" className="open" aria-label={project.name} title={[project.name, ...project.machines.map(machine => `${machine.name}: ${machine.folder}`)].join("\n")} aria-current={current ? "true" : undefined}><span className="cc-pdot" style={{ background: PROJECT_COLORS[project.i % PROJECT_COLORS.length] }} /><span className="tt">{label}</span>{!!project.agents && <span className="n">{project.agents}</span>}</button><Pin id={project.name} pinned={pinned} toggle={pin} /></div>;
  return <div className="cc-pjthreads">{projectNode}{threads.map(thread => { const title = thread.title || `${thread.kind === "pr" ? "PR" : "Issue"} #${thread.number}`; return <div className="cc-si cc-chat cc-thread" key={thread.chat_id}><button type="button" className="open" title={`${title} (${thread.project})`} aria-current={activeChat === thread.chat_id ? "true" : undefined} onClick={event => { event.stopPropagation(); openThread(thread.chat_id); }}><span className="tt">{title}</span></button>{running.has(thread.chat_id) && <Spinner />}</div>; })}</div>;
}
