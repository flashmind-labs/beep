import { AG, type AgentRow } from "./agents.ts";
import { sessionVisible } from "./closing.ts";
import { conversationHistory, conversationKey, isConversationKey, nextConversation } from "./conversation-state.ts";
import { loadSessions, sessions } from "./sessions.ts";
import { withoutCopies } from "./move-store.ts";
import { S } from "../bar/core.ts";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentView } from "../ui/document-root.tsx";
import { ConversationSwitcher } from "./conversation-switcher-view.tsx";
import { openSheet } from "../ui/parts.ts";

interface SwitcherOptions {
  rows?(): readonly AgentRow[];
  current?(): AgentRow | null;
  open(session: AgentRow): void;
}

/// Installed once per window, ahead of composer and terminal key handlers.
export function initConversationSwitcher(options: SwitcherOptions) {
  let picker: { close(): void; rows: AgentRow[]; index: number; draw(): void } | null = null;
  const current = () => options.current ? options.current() : S.view === "agent" && !AG.draft ? AG.page?.session ?? null : null;
  const available = () => {
    const on = current();
    return conversationHistory.order(withoutCopies([...(options.rows?.() ?? []), ...sessions, ...(on ? [on] : [])]).filter(sessionVisible), on);
  };
  const refresh = () => { void loadSessions().catch(e => console.warn("reading active conversations:", e)); };
  refresh();

  function finish(commit: boolean) {
    const p = picker;
    if (!p) return;
    const selected = p.rows[p.index];
    // A close or move arriving while choosing must not reopen a stale row.
    const live = commit && selected ? available().find(s => conversationKey(s) === conversationKey(selected)) : null;
    p.close();
    const on = current();
    if (live && (!on || conversationKey(live) !== conversationKey(on))) options.open(live);
  }

  function begin() {
    if (document.querySelector('[role="dialog"]')) return false;
    const rows = available();
    refresh();
    if (!rows.length) return false;
    let view: ReturnType<typeof createDocumentView> | null = null;
    const { root, sheet, close } = openSheet("Switch active conversation", "am-sheet conversation-switcher", { onClose: () => { view?.unmount(); view = null; picker = null; } });
    root.classList.add("conversation-scrim");
    view = createDocumentView(sheet);
    const on = current(), currentKey = on ? conversationKey(on) : null;
    const p = {
      close, rows, index: currentKey ? rows.findIndex(s => conversationKey(s) === currentKey) : -1,
      draw() {
        if (!view) return;
        flushSync(() => view?.render(createElement(ConversationSwitcher, {
          rows, index: p.index, current: currentKey, choose: (index: number) => { p.index = index; finish(true); },
        })));
      },
    };
    picker = p;
    p.draw();
    return true;
  }

  document.addEventListener("keydown", e => {
    if (e.isComposing) return;

    if (isConversationKey(e)) {
      if (!picker && !begin()) return;
      e.preventDefault(); e.stopImmediatePropagation();
      picker!.index = nextConversation(picker!.index, picker!.rows.length, e.shiftKey);
      picker!.draw();
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
