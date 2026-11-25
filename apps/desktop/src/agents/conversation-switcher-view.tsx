import { useLayoutEffect, useRef } from "react";
import { agentKind, folderName } from "./agent-kind.ts";
import { type ConversationChoice, conversationKey, conversationState } from "./conversation-state.ts";
import { windowHooks } from "../bar/core.ts";
import { HarnessIcon } from "../ui/icons.tsx";

export function ConversationSwitcher({ rows, index, current, choose }: { rows: ConversationChoice[]; index: number; current: string | null; choose(index: number): void }) {
  const list = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { list.current?.focus(); }, []);
  useLayoutEffect(() => { if (index >= 0) list.current?.children[index]?.scrollIntoView({ block: "nearest" }); }, [index]);
  return <><header><b>Active conversations</b><small>Most recently used</small></header>
    <div ref={list} className="conversation-list" tabIndex={0} role="listbox" aria-label="Active conversations" aria-activedescendant={index >= 0 ? `conversation-choice-${index}` : undefined}>
      {rows.map(({ row: session, depth }, i) => {
        const kind = agentKind(session.agent), project = windowHooks?.sessionProject?.(session)?.label || folderName(session.cwd);
        const title = session.title || project || kind.name;
        return <button key={conversationKey(session)} type="button" className={depth ? "conversation-choice sub" : "conversation-choice"} style={depth ? { marginInlineStart: `${depth * 28}px` } : undefined} tabIndex={-1} id={`conversation-choice-${i}`} title={[title, project, kind.name, session.machine_name].filter(Boolean).join(" · ")} role="option" aria-selected={i === index} onClick={() => choose(i)}>
          <span className="conversation-mark" aria-hidden="true"><HarnessIcon agent={session.agent} className="conversation-icon" /><i className={`conversation-dot ${session.status}`} /></span>
          <span className="conversation-label"><b>{title}</b>{!depth && <small>{[project, kind.name].filter(Boolean).join(" · ")}</small>}</span><span className="conversation-state">{conversationState(session)}{conversationKey(session) === current && <small>Current</small>}</span>
        </button>;
      })}
    </div><footer><span>Hold Option, press Tab to cycle</span><span>Release to open · Shift to reverse · Esc to cancel</span></footer></>;
}
