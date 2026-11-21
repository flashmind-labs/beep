// A chat's threads: the sessions its agent started, which the user sees as
// parts of the chat. No DOM here.
import type { AgentSession } from "./agents.ts";
import { parentRow } from "./nesting-state.ts";
import { AGENT_ACTIVE } from "./sessionmenu-state.ts";

/// How a thread stands: `done` when its last turn ended and it waits.
export type ThreadState = "working" | "asking" | "done" | "ended";

/// What a thread's row says after its name: what it asks the user, else
/// the first line of what it last said, as inline HTML.
export type ThreadDetail = { kind: "question"; text: string } | { kind: "reply"; html: string } | null;

export interface Thread {
  row: AgentSession;
  name: string;
  state: ThreadState;
  detail: ThreadDetail;
}

/// What the user types to have a chat's agent start a thread, as agents are
/// told (`BRIEF` in the node).
export const NEW_THREAD = "New thread: ";

/// The threads of the chat `main`, oldest first: its direct children, but
/// for the reviewers and background agents, which work for the agent alone.
export function threadsOf(rows: readonly AgentSession[], main: Pick<AgentSession, "machine_id" | "id">): Thread[] {
  const list = [...rows];
  const parent = list.find(row => row.machine_id === main.machine_id && row.id === main.id);
  if (!parent) return [];

  return list
    .filter(row => !row.reviewer && !row.background && row !== parent && parentRow(list, row) === parent)
    .sort((a, b) => a.started_at - b.started_at)
    .map(row => ({ row, name: row.title || row.agent, state: threadState(row.status), detail: threadDetail(row) }));
}

function threadState(status: string): ThreadState {
  if (status === "asking") return "asking";
  if (status === "ended") return "ended";
  return AGENT_ACTIVE.has(status) ? "working" : "done";
}

function threadDetail(row: AgentSession): ThreadDetail {
  if (row.status === "asking" && row.question) return { kind: "question", text: row.question.text };
  return row.reply_html ? { kind: "reply", html: row.reply_html } : null;
}
