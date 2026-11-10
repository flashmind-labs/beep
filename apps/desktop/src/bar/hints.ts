import { attachedCount } from "../chat/attach.ts";
import { drawQueue } from "../chat/chat.ts";
import { forRoutine } from "../chat/approval-cards.ts";
import { $, bar, here, PANE, queued, S } from "./core.ts";
import { linkChip } from "../settings/server-state.ts";
import { liveParts, LV } from "./live.ts";
import { modelChipLabel } from "../settings/model.ts";
import { waitingQuestion } from "./work.ts";
import { ICON } from "../ui/icons.tsx";
import type { HintViewModel } from "./hints-view.tsx";

// ---------- Hints ----------

/// A key and what it does, as the hint row and the shortcuts sheet show it.
export type Hint = readonly string[];

export function hintModel(items: readonly Hint[]): HintViewModel {
  return { items: forPane(items), window: !PANE && S.view === "chat", model: null, link: linkChip(S.server, S.link, S.signedOut) };
}

/// The window does not hide and has no live mode, and ⌘K searches there.
export function forPane(items: readonly Hint[]) {
  if (!PANE) return items;
  return items
    .filter(([, t]) => t !== "to hide" && t !== "live" && t !== "talk")
    .map(([k, t]) => k === "⌘K" ? [k, "search"] : [k, t]);
}
export function chatHints() {
  const e = here();
  const chip = modelChipLabel();
  const make = (items: readonly Hint[]): HintViewModel => ({ items: forPane(items), window: !PANE && S.view === "chat", model: chip, link: linkChip(S.server, S.link, S.signedOut) });
  const approval = e?.approval, question = !approval && !e?.run && waitingQuestion(S.session);

  if (approval || question) {
    const grant = question ? forRoutine(question.action) : approval?.grant;
    return make([[`⌘${ICON.ret}`, "allow"], ...(grant ? [] : [[`⇧⌘${ICON.ret}`, "this session"], [`⌥⌘${ICON.ret}`, "always"]]), ["⌘D", "deny"], ["esc", "to hide"]]);
  }

  const editQueued: Hint[] = queued.get(S.session)?.list.length ? [["↑", "edit queued"]] : [];
  if (e?.run) return make([[ICON.ret, "queue"], ...editQueued, ["⌘.", "stop"], ["⌘K", "chats"], ["esc", "to hide"]]);
  if (queued.get(S.session)?.held) return make([[`⌘${ICON.ret}`, "send queued"], ...editQueued, ["⌘N", "new agent"], ["⌘K", "chats"], ["esc", "to hide"]]);
  if (attachedCount()) return make([[ICON.ret, "send"], ["⌘V", "paste"], ["⌫", "remove last"], ["esc", "to hide"]]);
  const typed = $<HTMLTextAreaElement>("#ask")?.value;
  if (typed?.includes("\n")) return make([[ICON.ret, "send"], [`⇧${ICON.ret}`, "new line"], ["⌘K", "chats"], ["esc", "to hide"]]);
  if (typed?.trim()) return make([[ICON.ret, "ask"], ...(S.link === "linked" ? [["⌘R", "research"] as Hint] : []), ["esc", "to hide"]]);
  if (PANE) return make([["⌘K", "search"], ["⌘N", "new agent"], [`⇧${ICON.ret}`, "new line"], ["⌥ ⌥", "quick chat"]]);
  if (!knowsHotkey()) return make([["⌥ ⌥", "open from anywhere"], ["⌘N", "new agent"], ["⌘K", "chats"], ["esc", "to hide"]]);
  return make([["⌘N", "new agent"], ["⌘K", "chats"], ...(S.live ? [["tap ⌥", "live"], ["hold ⌥", "talk"]] as Hint[] : []), ["⌘/", "shortcuts"], ["esc", "to hide"]]);
}
// Shown from first launch until the bar is first opened with a double tap of Option.
export const KNOWS_HOTKEY = "beep.knowsHotkey";
export function knowsHotkey() { try { return localStorage.getItem(KNOWS_HOTKEY) === "1"; } catch { return false; } }
export function learnedHotkey() { try { localStorage.setItem(KNOWS_HOTKEY, "1"); } catch {} }
export function refreshHints() {
  if (S.view === "chat" && bar.querySelector(".dhints")) drawQueue();
}

export function currentChatHints(): HintViewModel {
  if (LV.on && LV.view && $("#ldock")) return { items: forPane(liveParts(LV.view).keys), window: !PANE, model: null, link: linkChip(S.server, S.link, S.signedOut) };
  return chatHints();
}

// ---------- Live mode keys ----------

/// What can be said in live mode, and its keys, in the
/// sheet ⌘/ opens over live mode.
export const LIVE_SAY: Hint[] = [["“send”", "Send now"], ["“stop”", "Send now, or stop Beep speaking"]];
export const LIVE_TALK: Hint[] = [["tap ⌥", "Leave, or stop Beep"], ["hold ⌥", "Talk, or interrupt Beep"], ["esc", "Cancel the reply, or leave"], ["any key", "Leave and type instead"], ["⌥ ⌥", "Over the orb: bring Beep back. In Beep: leave"]];
