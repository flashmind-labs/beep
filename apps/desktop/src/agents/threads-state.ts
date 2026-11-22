// An assistant chat's threads: the sessions its agent started, which the user
// sees as parts of the chat. No DOM here.
import type { AgentSession } from "./agents.ts";
import { parentRow, startedBy } from "./nesting-state.ts";

/// What a thread's row says after its name: what it asks the user, else
/// the first line of what it last said, as inline HTML.
export type ThreadDetail = { kind: "question"; text: string } | { kind: "reply"; html: string } | null;

export interface Thread {
  row: AgentSession;
  name: string;
  detail: ThreadDetail;
}

/// What the user types to have a chat's agent start a thread, as agents are
/// told (`BRIEF` in the node).
export const NEW_THREAD = "New thread: ";

/// The assistant chat `session` is part of: itself, or the one whose thread it is or works
/// under. Null for a reviewer and for every session no assistant chat started.
export function chatOf<S extends Partial<Pick<AgentSession, "assistant" | "reviewer" | "parent">>>(rows: readonly AgentSession[], session: S): S | AgentSession | null {
  if (session.reviewer) return null;
  let at: S | AgentSession | undefined = session;
  // Agents start agents three deep at most.
  for (let depth = 0; at && !at.assistant && depth < 3; depth += 1) at = startedBy(rows, at);

  return at?.assistant ? at : null;
}

/// The threads of the chat `main`, oldest first: its direct children, but
/// for the reviewers and background agents, which work for the agent alone.
export function threadsOf(rows: readonly AgentSession[], main: Pick<AgentSession, "machine_id" | "id">): Thread[] {
  const list = [...rows];
  const parent = list.find(row => row.machine_id === main.machine_id && row.id === main.id);
  if (!parent) return [];

  return list
    .filter(row => !row.reviewer && !row.background && row !== parent && parentRow(list, row) === parent)
    .sort((a, b) => a.started_at - b.started_at)
    .map(row => ({ row, name: row.title || row.agent, detail: threadDetail(row) }));
}

function threadDetail(row: AgentSession): ThreadDetail {
  if (row.status === "asking" && row.question) return { kind: "question", text: row.question.text };
  return row.reply_html ? { kind: "reply", html: row.reply_html } : null;
}
