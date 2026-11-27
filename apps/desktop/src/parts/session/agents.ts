import { AG } from "../../stores/agents/agents-state.ts";
import { openScreen } from "../center/nav.ts";
import { onPhone, WINDOWS } from "../../ui/platform.ts";
// agents/: coding agents in the bar: the start form, the picker, one
// session's log and messages, and every session. Entry: this file.
// Start a coding agent on one of the user's machines, follow it live and
// answer it: `agentnew` is the start form, `agent` one session, `agents`
// every session. They run in the bar and in the window's pane alike. This
// file holds their keys; the Every session list is in agents-list.ts.
import { chatOf } from "../../stores/agents/threads-state.ts";
import { sessions } from "../../stores/agents/sessions.ts";
import { newTerminal, newTerminalLike, showPaneTab } from "./media.ts";
import { leavePane } from "./pane-tabs.ts";
import { PANE, S } from "../../stores/bar/core.ts";
import { openAgentInWindow } from "../../stores/bar/hints.ts";
import { agentsList, openListed, pickable } from "./agents-list.ts";
import { closeOpenSession, submitAgentMessage } from "./session.tsx";
import { openEditorKey } from "./session-head.tsx";
import { toggleCommands, toggleLastCommand, toggleThoughts } from "./logkeys.ts";
import { jumpToStart, logJumpKey } from "./older.ts";
import { jumpToLatest } from "./logview.tsx";
import { pageDirection } from "./logkeys-state.ts";
import { toggleModelPicker } from "./modelpill.tsx";
import { interruptAgent, answerFirst } from "./messages.ts";
import { cycleMode } from "./modepill.tsx";
import { openChatWindow } from "../project/projectops.ts";
import { agentMessage, sessionBody, shownMessage } from "../../stores/agents/session-frame-state.ts";

/// Keys of the agent views; true when one was taken.
export function agentKeys(e: KeyboardEvent, cmd: boolean) {
  if (S.view === "agentnew") return false;

  if (S.view === "agent") {
    if (cmd && e.key === "Enter" && answerFirst(true)) { e.preventDefault(); return true; }
    if (cmd && e.key.toLowerCase() === "d" && answerFirst(false)) { e.preventDefault(); return true; }
    if (cmd && e.key === ".") { e.preventDefault(); interruptAgent(); return true; }

    // ⌘T a terminal in the pane, ⌘N one like the terminal on screen, ⌘1 focuses chat, ⌘2 to ⌘9 the pane's tabs
    // in strip order, ⌘O the session in the window from the bar, else the folder in the editor. ⌘U opens or folds every thought, ⌘Y
    // every command, ⇧⌘Y the last one, ⇧⌘M the model picker. They work from the field and from a terminal: none edits text.
    if (cmd && !e.altKey && (WINDOWS || !e.ctrlKey) && !e.isComposing) {
      const k = e.key.toLowerCase();
      if (k === "w" && e.shiftKey && !AG.draft && AG.open) { e.preventDefault(); if (!e.repeat) void closeOpenSession(); return true; }
      if (k === "t" && !e.shiftKey) { e.preventDefault(); newTerminal(undefined, !!AG.draft); return true; }
      if (k === "n" && !e.shiftKey && newTerminalLike()) { e.preventDefault(); return true; }
      if (k === "n" && e.shiftKey && chatWindowHere()) { e.preventDefault(); return true; }
      if (k === "u" && !e.shiftKey) { e.preventDefault(); toggleThoughts(); return true; }
      if (k === "m" && e.shiftKey) { e.preventDefault(); toggleModelPicker(); return true; }
      if (k === "y") { e.preventDefault(); (e.shiftKey ? toggleLastCommand : toggleCommands)(); return true; }

      if (!e.shiftKey && k === "1") {
        e.preventDefault();
        leavePane();
        const body = sessionBody(), chat = shownMessage() ?? body;
        if (chat && chat === body && !chat.hasAttribute("tabindex")) chat.setAttribute("tabindex", "-1");
        chat?.focus();
        return true;
      }

      if (!e.shiftKey && k >= "2" && k <= "9" && k.length === 1) { e.preventDefault(); showPaneTab(Number(k) - 2); return true; }
      if (k === "o" && !e.shiftKey) {
        e.preventDefault();
        if (!PANE && AG.page && !AG.draft) void openAgentInWindow(AG.page.session); else openEditorKey();
        return true;
      }
    }

    const target = e.target, field = agentMessage.current, inField = !!field && target === field;
    const body = sessionBody();
    const page = pageDirection(e, !!body && ((inField && !field.value) || target === body || target === document.body));

    if (page && body) {
      e.preventDefault();
      body.stuck = false;
      body.scrollTop += page * body.clientHeight * 0.9;
      return true;
    }

    // Home and ⌘↑ go to the first entry of the log, End and ⌘↓ to the last,
    // unless the field has text to move through.
    const jump = logJumpKey(e, cmd);
    if (jump) { e.preventDefault(); void (jump === "start" ? jumpToStart() : jumpToLatest()); return true; }
    if (e.key === "Tab" && e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey && inField && cycleMode()) { e.preventDefault(); return true; }

    // On a phone Return starts a new line; the Send button sends.
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing && inField && !document.body?.classList.contains("mobile")) {
      e.preventDefault();
      submitAgentMessage(field);
      return true;
    }

    return false;
  }

  if (S.view === "agents") {
    const list = agentsList.getState(), rows = pickable(list), n = rows.length;
    if (!n) return false;

    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      agentsList.setState({ selected: (list.selected + (e.key === "ArrowDown" ? 1 : n - 1)) % n });
      return true;
    }

    if (e.key === "Enter") { e.preventDefault(); openListed(rows[list.selected].session); return true; }
  }

  return false;
}

/// ⇧⌘N on an assistant chat or one of its threads: the chat with its threads in a window of
/// its own.
function chatWindowHere() {
  const s = AG.draft ? null : AG.page?.session, chat = s ? chatOf(sessions, s) : null;
  if (!chat?.assistant || onPhone()) return false;
  openChatWindow(chat);
  return true;
}

/// ⌘N beside a chat's threads: a new thread of the chat, asked for in the chat's field. False
/// elsewhere, and for a chat that ended.
export function newThreadHere() {
  const s = AG.draft ? null : AG.page?.session, chat = s ? chatOf(sessions, s) : null;
  if (!chat?.assistant || chat.status === "ended") return false;
  openScreen({ screen: "newthread", chat });
  return true;
}
