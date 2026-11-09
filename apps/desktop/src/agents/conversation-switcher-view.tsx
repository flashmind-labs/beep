import { useLayoutEffect, useRef } from "react";
import { agentKind, folderName, type AgentRow } from "./agents.ts";
import { conversationKey } from "./conversation-state.ts";
import { windowHooks } from "../bar/core.ts";
import { harnessInner } from "../ui/harnessicon.ts";

export function ConversationSwitcher({ rows, index, current, choose }: { rows: AgentRow[]; index: number; current: string | null; choose(index: number): void }) {
  const list = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { list.current?.focus(); }, []);
  useLayoutEffect(() => { if (index >= 0) list.current?.children[index]?.scrollIntoView({ block: "nearest" }); }, [index]);
  return <><header><b>Active conversations</b><small>Most recently used</small></header>
    <div ref={list} className="conversation-list" tabIndex={0} role="listbox" aria-label="Active conversations" aria-activedescendant={index >= 0 ? `conversation-choice-${index}` : undefined}>
      {rows.map((session, i) => {
        const kind = agentKind(session.agent), project = windowHooks?.sessionProject?.(session)?.label || folderName(session.cwd);
        const state = session.status === "asking" ? "Needs you" : session.status === "idle" ? "Ready" : session.status === "waiting" ? "Waiting" : session.status === "starting" ? "Starting" : "Working";
        const title = session.title || project || kind.name;
        return <button key={conversationKey(session)} type="button" className="conversation-choice" tabIndex={-1} id={`conversation-choice-${i}`} title={[title, project, kind.name, session.machine_name].filter(Boolean).join(" · ")} role="option" aria-selected={i === index} onClick={() => choose(i)}>
          <span className="conversation-mark" aria-hidden="true"><span className="conversation-icon" dangerouslySetInnerHTML={{ __html: harnessInner(session.agent) }} /><i className={`conversation-dot ${session.status}`} /></span>
          <span className="conversation-label"><b>{title}</b><small>{[project, kind.name].filter(Boolean).join(" · ")}</small></span><span className="conversation-state">{state}{conversationKey(session) === current && <small>Current</small>}</span>
        </button>;
      })}
    </div><footer><span>Hold Option, press Tab to cycle</span><span>Release to open · Shift to reverse · Esc to cancel</span></footer></>;
}
