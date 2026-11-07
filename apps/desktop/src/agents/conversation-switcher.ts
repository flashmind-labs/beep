import { AG, agentKind, folderName, type AgentRow } from "./agents.ts";
import { sessionVisible } from "./closing.ts";
import { conversationHistory, conversationKey, isConversationKey, nextConversation } from "./conversation-state.ts";
import { loadSessions, sessions } from "./sessions.ts";
import { withoutCopies } from "./move-store.ts";
import { S, windowHooks } from "../bar/core.ts";
import { el } from "../ui/dom.ts";
import { harnessInner } from "../ui/harnessicon.ts";
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
    const { root, sheet, close } = openSheet("Switch active conversation", "am-sheet conversation-switcher", { onClose: () => { picker = null; } });
    root.classList.add("conversation-scrim");
    const list = el("div", "conversation-list");
    list.tabIndex = 0;
    list.setAttribute("role", "listbox");
    list.setAttribute("aria-label", "Active conversations");
    const on = current(), currentKey = on ? conversationKey(on) : null;
    const p = {
      close, rows, index: currentKey ? rows.findIndex(s => conversationKey(s) === currentKey) : -1,
      draw() {
        list.replaceChildren(...rows.map((s, i) => {
          const kind = agentKind(s.agent);
          const project = windowHooks?.sessionProject?.(s)?.label || folderName(s.cwd);
          const state = s.status === "asking" ? "Needs you" : s.status === "idle" ? "Ready" : s.status === "waiting" ? "Waiting" : s.status === "starting" ? "Starting" : "Working";
          const icon = el("span", "conversation-icon");
          icon.innerHTML = harnessInner(s.agent);
          const mark = el("span", "conversation-mark", icon, el("i", `conversation-dot ${s.status}`));
          mark.setAttribute("aria-hidden", "true");
          const title = s.title || project || kind.name;
          const row = el("button", "conversation-choice", mark,
            el("span", "conversation-label", el("b", "", title), el("small", "", [project, kind.name].filter(Boolean).join(" · "))),
            el("span", "conversation-state", state, conversationKey(s) === currentKey ? el("small", "", "Current") : null));
          row.type = "button";
          row.tabIndex = -1;
          row.setAttribute("id", `conversation-choice-${i}`);
          row.title = [title, project, kind.name, s.machine_name].filter(Boolean).join(" · ");
          row.setAttribute("role", "option");
          row.setAttribute("aria-selected", String(i === p.index));
          row.onclick = () => { p.index = i; finish(true); };
          return row;
        }));
        if (p.index >= 0) {
          list.setAttribute("aria-activedescendant", `conversation-choice-${p.index}`);
          list.children[p.index]?.scrollIntoView({ block: "nearest" });
        }
      },
    };
    picker = p;
    sheet.append(el("header", "", el("b", "", "Active conversations"), el("small", "", "Most recently used")), list,
      el("footer", "", el("span", "", "Hold Option, press Tab to cycle"), el("span", "", "Release to open · Shift to reverse · Esc to cancel")));
    list.focus();
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
