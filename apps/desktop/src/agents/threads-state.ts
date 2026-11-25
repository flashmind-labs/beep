// An assistant chat's threads: the sessions its agent started, which the user
// sees as parts of the chat. No DOM here.
import type { AgentSession } from "./agents-state.ts";
import { startedBy, starters } from "./nesting-state.ts";

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

/// Each chat's threads, oldest first, by the chat's row in `rows`: its direct children, but for
/// the reviewers and background agents, which work for the agent alone. The rows are read
/// once for all the chats.
export function threadsByChat(rows: readonly AgentSession[]): Map<AgentSession, Thread[]> {
  const from = starters(rows), threads = new Map<AgentSession, Thread[]>();

  for (const row of [...rows].sort((a, b) => a.started_at - b.started_at)) {
    const chat = row.reviewer || row.background ? undefined : from(row);
    if (!chat || chat === row) continue;
    const list = threads.get(chat) ?? [];
    list.push({ row, name: row.title || row.agent, detail: threadDetail(row) });
    threads.set(chat, list);
  }

  return threads;
}

/// How many of a chat's threads ask the user or work.
export function chatActivity(threads: readonly Thread[]): { asking: number; working: number } {
  return {
    asking: threads.filter(({ row }) => row.status === "asking").length,
    working: threads.filter(({ row }) => WORKING.has(row.status)).length,
  };
}

/// How many chats wait on the user: the chat asks, or one of its threads does.
export function chatsAsking(rows: readonly AgentSession[]): number {
  const threads = threadsByChat(rows);
  return rows.filter(row => row.assistant && !row.reviewer && (row.status === "asking" || chatActivity(threads.get(row) ?? []).asking > 0)).length;
}

const WORKING = new Set(["working", "running", "starting"]);

/// The list beside a chat that has threads, as for a project's sessions: `chat` and its running
/// threads, then the ended ones, newest first, with the chat ahead of them once it ended. Null
/// for `session` when it is part of no chat, or of one without threads.
export function chatRail(rows: readonly AgentSession[], session: Partial<Pick<AgentSession, "assistant" | "reviewer" | "parent">> & Pick<AgentSession, "machine_id" | "id">, always = false): { chat: AgentSession; running: AgentSession[]; past: AgentSession[] } | null {
  const found = chatOf(rows, session);
  const chat = found && rows.find(row => row.machine_id === found.machine_id && row.id === found.id);
  const threads = chat ? threadsByChat(rows).get(chat)?.map(thread => thread.row) ?? [] : [];
  if (!chat || (!threads.length && !always)) return null;
  const ended = (row: AgentSession) => row.status === "ended";
  const past = threads.filter(ended).sort((a, b) => b.last_activity - a.last_activity);

  return {
    chat,
    running: [...(ended(chat) ? [] : [chat]), ...threads.filter(row => !ended(row))],
    past: ended(chat) ? [chat, ...past] : past,
  };
}

/// The folders under `session`'s own that its threads work in, by their names there, which its
/// files leave out: each thread shows its own.
export function threadFolders(rows: readonly AgentSession[], session: Pick<AgentSession, "machine_id" | "id" | "cwd">): Set<string> {
  const listed = rows.find(row => row.machine_id === session.machine_id && row.id === session.id);
  const threads = listed ? threadsByChat(rows).get(listed) ?? [] : [];
  const names = threads.flatMap(({ row }) => {
    const name = row.cwd.startsWith(session.cwd) ? /^[\\/]([^\\/]+)/.exec(row.cwd.slice(session.cwd.length))?.[1] : null;
    return name ? [name] : [];
  });

  return new Set(names);
}

function threadDetail(row: AgentSession): ThreadDetail {
  if (row.status === "asking" && row.question) return { kind: "question", text: row.question.text };
  return row.reply_html ? { kind: "reply", html: row.reply_html } : null;
}
