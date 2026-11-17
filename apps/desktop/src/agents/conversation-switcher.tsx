import { AG, type AgentRow } from "./agents.ts";
import { sessionVisible } from "./closing.ts";
import { type ConversationChoice, type ConversationScope, conversationHistory, conversationKey, conversationScope, firstConversation, groupConversations, nextConversation } from "./conversation-state.ts";
import { loadSessions, parentSession, sessions } from "./sessions.ts";
import { withoutCopies } from "./move-store.ts";
import { S, windowHooks } from "../bar/core.ts";
import { flushSync } from "react-dom";
import { ConversationSwitcher } from "./conversation-switcher-view.tsx";
import { dialogOpen, openSheet } from "../ui/parts.tsx";

interface SwitcherOptions {
  rows?(): readonly AgentRow[];
  current?(): AgentRow | null;
  open(session: AgentRow): void;
}

/// Installed once per window, ahead of composer and terminal key handlers.
export function initConversationSwitcher(options: SwitcherOptions) {
  let picker: { close(): void; scope: ConversationScope; rows: ConversationChoice[]; index: number; draw(): void } | null = null;
  const current = () => options.current ? options.current() : S.view === "agent" && !AG.draft ? AG.page?.session ?? null : null;
  const projectOf = (row: AgentRow) => windowHooks?.sessionProject?.(row)?.name ?? null;
  /// The conversations `scope` cycles. A project's are those of the session on screen, so
  /// there are none away from one.
  const available = (scope: ConversationScope) => {
    const on = current(), project = scope === "project" && on ? projectOf(on) : null;
    if (scope === "project" && !project) return [];
    const all = withoutCopies([...(options.rows?.() ?? []), ...sessions, ...(on ? [on] : [])]).filter(sessionVisible);
    const rows = conversationHistory.order(project ? all.filter(row => projectOf(row) === project) : all, on);
    return groupConversations(rows, row => parentSession(row) ?? null);
  };
  const refresh = () => { void loadSessions().catch(e => console.warn("reading active conversations:", e)); };
  refresh();

  function finish(commit: boolean) {
    const p = picker;
    if (!p) return;
    const selected = p.rows[p.index]?.row;
    // A close or move arriving while choosing must not reopen a stale row.
    const live = commit && selected ? available(p.scope).find(c => conversationKey(c.row) === conversationKey(selected))?.row : null;
    p.close();
    const on = current();
    if (live && (!on || conversationKey(live) !== conversationKey(on))) options.open(live);
  }

  function begin(scope: ConversationScope) {
    if (dialogOpen()) return false;
    const rows = available(scope);
    refresh();
    if (!rows.length) return false;
    let open = true;
    const { close, render } = openSheet("Switch active conversation", "am-sheet conversation-switcher", { scrim: "conversation-scrim", onClose: () => { open = false; picker = null; } });
    const on = current(), currentKey = on ? conversationKey(on) : null;
    const p = {
      close, scope, rows, index: currentKey ? rows.findIndex(c => conversationKey(c.row) === currentKey) : -1,
      draw() {
        if (!open) return;
        flushSync(() => render(
          <ConversationSwitcher scope={scope} rows={rows} index={p.index} current={currentKey} choose={(index: number) => { p.index = index; finish(true); }} />,
        ));
      },
    };
    picker = p;
    p.draw();
    return true;
  }

  document.addEventListener("keydown", e => {
    if (e.isComposing) return;

    const scope = conversationScope(e);
    if (scope) {
      const fresh = !picker;
      if (fresh && !begin(scope)) return;
      if (!picker) return;
      e.preventDefault(); e.stopImmediatePropagation();
      picker.index = fresh ? firstConversation(picker.rows, picker.index, e.shiftKey) : nextConversation(picker.index, picker.rows.length, e.shiftKey);
      picker.draw();
      return;
    }

    if (!picker) return;
    e.stopImmediatePropagation();
    e.preventDefault();

    if (e.key === "Escape") finish(false);
    else if (e.key === "Enter") finish(true);
    else if (["ArrowUp", "ArrowDown", "Tab"].includes(e.key)) {
      picker.index = nextConversation(picker.index, picker.rows.length, e.key === "ArrowUp" || (e.key === "Tab" && e.shiftKey));
      picker.draw();
    }
  }, true);
  document.addEventListener("keyup", e => {
    // Letting go of the key the switcher opened with opens the choice.
    const [key, held] = picker?.scope === "project" ? ["Control", e.ctrlKey] : ["Alt", e.altKey];
    if (picker && (e.key === key || !held)) {
      e.preventDefault(); e.stopImmediatePropagation();
      finish(e.key === key);
    }
  }, true);
  window.addEventListener("blur", () => finish(false));
  document.addEventListener("visibilitychange", () => { if (document.hidden) finish(false); });
}
