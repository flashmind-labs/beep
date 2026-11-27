import { AG, type AgentRow } from "../../stores/agents/agents-state.ts";
import { sessionVisible } from "../../stores/agents/closing.ts";
import { type ConversationChoice, conversationHistory, conversationKey, firstConversation, groupConversations, isConversationKey, nextConversation } from "../../stores/agents/conversation-state.ts";
import { ensureSessions, parentSession, sessions } from "../../stores/agents/sessions.ts";
import { withoutCopies } from "../../stores/agents/move-store.ts";
import { S } from "../../stores/bar/core.ts";
import { flushSync } from "react-dom";
import { ConversationSwitcher } from "./conversation-switcher-view.tsx";
import { dialogOpen, openSheet } from "../../ui/parts.tsx";

interface SwitcherOptions {
  rows?(): readonly AgentRow[];
  current?(): AgentRow | null;
  open(session: AgentRow): void;
}

/// Installed once per window, ahead of composer and terminal key handlers.
export function initConversationSwitcher(options: SwitcherOptions) {
  let picker: { close(): void; rows: ConversationChoice[]; index: number; draw(): void } | null = null;
  const current = () => options.current ? options.current() : S.view === "agent" && !AG.draft ? AG.page?.session ?? null : null;
  const available = () => {
    const on = current();
    const rows = conversationHistory.order(withoutCopies([...(options.rows?.() ?? []), ...sessions, ...(on ? [on] : [])]).filter(sessionVisible), on);
    return groupConversations(rows, row => parentSession(row) ?? null);
  };
  ensureSessions().catch(e => console.warn("reading active conversations:", e));

  function finish(commit: boolean) {
    const p = picker;
    if (!p) return;
    const selected = p.rows[p.index]?.row;
    // A close or move arriving while choosing must not reopen a stale row.
    const live = commit && selected ? available().find(c => conversationKey(c.row) === conversationKey(selected))?.row : null;
    p.close();
    const on = current();
    if (live && (!on || conversationKey(live) !== conversationKey(on))) options.open(live);
  }

  function begin() {
    if (dialogOpen()) return false;
    const rows = available();
    if (!rows.length) return false;
    let open = true;
    const { close, render } = openSheet("Switch active conversation", "am-sheet conversation-switcher", { scrim: "conversation-scrim", onClose: () => { open = false; picker = null; } });
    const on = current(), currentKey = on ? conversationKey(on) : null;
    const p = {
      close, rows, index: currentKey ? rows.findIndex(c => conversationKey(c.row) === currentKey) : -1,
      draw() {
        if (!open) return;
        flushSync(() => render(
          <ConversationSwitcher rows={rows} index={p.index} current={currentKey} choose={(index: number) => { p.index = index; finish(true); }} />,
        ));
      },
    };
    picker = p;
    p.draw();
    return true;
  }

  document.addEventListener("keydown", e => {
    if (e.isComposing) return;

    if (isConversationKey(e)) {
      const fresh = !picker;
      if (fresh && !begin()) return;
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
    if (picker && (e.key === "Alt" || !e.altKey)) {
      e.preventDefault(); e.stopImmediatePropagation();
      finish(e.key === "Alt");
    }
  }, true);
  window.addEventListener("blur", () => finish(false));
  document.addEventListener("visibilitychange", () => { if (document.hidden) finish(false); });
}
