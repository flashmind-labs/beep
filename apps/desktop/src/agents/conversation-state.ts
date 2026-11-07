import type { AgentRow } from "./agents.ts";

export const conversationKey = (s: Pick<AgentRow, "machine_id" | "agent" | "id">) => JSON.stringify([s.machine_id, s.agent, s.id]);

/// Visits change when a conversation opens, never while the picker cycles.
export class ConversationHistory {
  private visits: string[] = [];
  visit(s: AgentRow) {
    const key = conversationKey(s);
    this.visits = [key, ...this.visits.filter(k => k !== key)].slice(0, 100);
  }
  order(rows: readonly AgentRow[], current: AgentRow | null): AgentRow[] {
    const unique = new Map<string, AgentRow>();
    for (const row of rows) {
      unique.set(conversationKey(row), row);
    }
    const rank = (s: AgentRow) => {
      const key = conversationKey(s);
      if (current && key === conversationKey(current)) return -1;
      const i = this.visits.indexOf(key);
      return i < 0 ? Infinity : i;
    };
    return [...unique.values()].filter(row => row.status !== "ended" && row.status !== "closing").sort((a, b) => rank(a) - rank(b) || (b.last_activity ?? b.started_at ?? 0) - (a.last_activity ?? a.started_at ?? 0));
  }
}
export const conversationHistory = new ConversationHistory();

export function nextConversation(index: number, count: number, backwards: boolean) {
  if (!count) return -1;
  if (index < 0) return backwards ? count - 1 : 0;
  return (index + (backwards ? -1 : 1) + count) % count;
}
export function isConversationKey(e: Pick<KeyboardEvent, "key" | "altKey" | "ctrlKey" | "metaKey" | "isComposing">) {
  return e.key === "Tab" && e.altKey && !e.ctrlKey && !e.metaKey && !e.isComposing;
}
