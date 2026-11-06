import { attachedCount } from "../chat/attach.ts";
import { focusAsk, forRoutine, stop } from "../chat/chat.ts";
import { $, bar, here, PANE, queued, S } from "./core.ts";
import { linkChip } from "../settings/server-state.ts";
import { liveParts, LV } from "./live.ts";
import { modelChip } from "../settings/model.ts";
import { waitingQuestion } from "./work.ts";
import { stopFace } from "../agents/turnrows.ts";
import { h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";

// ---------- Hints ----------

/// A key and what it does, as the hint row and the shortcuts sheet show it.
export type Hint = readonly string[];

export function hints(items: readonly Hint[], extra = "") {
  items = forPane(items);
  const st = linkChip(S.server, S.link, S.signedOut);
  const link = st ? `<button class="st"><span class="ddot ${st[0]}"></span>${st[1]}</button>` : "";
  // The first two say the most; the rest give way when the row is narrow.
  // The bar's chat opens in the window from here, before the model.
  const win = !PANE && S.view === "chat" ? '<button class="win" data-act="openWindow"><kbd>⌘O</kbd>open in window</button>' : "";
  return `<div class="dhints">${items.map(([k, t], i) => `<span${i >= 2 ? ' class="lo"' : ""}><kbd>${k}</kbd>${t}</span>`).join("")}${win}${extra}${link}</div>`;
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
  const chip = modelChip();
  const approval = e?.approval, question = !approval && !e?.run && waitingQuestion(S.session);
  if (approval || question) {
    const grant = question ? forRoutine(question.action) : approval?.grant;
    return hints([[`⌘${ICON.ret}`, "allow"], ...(grant ? [] : [[`⇧⌘${ICON.ret}`, "this session"], [`⌥⌘${ICON.ret}`, "always"]]), ["⌘D", "deny"], ["esc", "to hide"]], chip);
  }
  const editQueued: Hint[] = queued.get(S.session)?.list.length ? [["↑", "edit queued"]] : [];
  if (e?.run) return hints([[ICON.ret, "queue"], ...editQueued, ["⌘.", "stop"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (queued.get(S.session)?.held) return hints([[`⌘${ICON.ret}`, "send queued"], ...editQueued, ["⌘N", "new agent"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (attachedCount()) return hints([[ICON.ret, "send"], ["⌘V", "paste"], ["⌫", "remove last"], ["esc", "to hide"]], chip);
  const typed = $<HTMLTextAreaElement>("#ask")?.value;
  if (typed?.includes("\n")) return hints([[ICON.ret, "send"], [`⇧${ICON.ret}`, "new line"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  if (typed?.trim()) return hints([[ICON.ret, "ask"], ...(S.link === "linked" ? [["⌘R", "research"] as Hint] : []), ["esc", "to hide"]], chip);
  if (PANE) return hints([["⌘K", "search"], ["⌘N", "new agent"], [`⇧${ICON.ret}`, "new line"], ["⌥ ⌥", "quick chat"]], chip);
  if (!knowsHotkey()) return hints([["⌥ ⌥", "open from anywhere"], ["⌘N", "new agent"], ["⌘K", "chats"], ["esc", "to hide"]], chip);
  return hints([["⌘N", "new agent"], ["⌘K", "chats"], ...(S.live ? [["tap ⌥", "live"], ["hold ⌥", "talk"]] as Hint[] : []), ["⌘/", "shortcuts"], ["esc", "to hide"]], chip);
}
// Shown from first launch until the bar is first opened with a double tap of Option.
export const KNOWS_HOTKEY = "beep.knowsHotkey";
export function knowsHotkey() { try { return localStorage.getItem(KNOWS_HOTKEY) === "1"; } catch { return false; } }
export function learnedHotkey() { try { localStorage.setItem(KNOWS_HOTKEY, "1"); } catch {} }
export function refreshHints() {
  const old = bar.querySelector(":scope > .dhints");
  if (!old || S.view !== "chat") return;
  old.replaceWith(h(LV.on && LV.view && $("#ldock") ? hints(liveParts(LV.view).keys) : chatHints()));
  drawStop();
}
/// The Stop button beside the prompt while a reply runs, as ⌘. does.
export function drawStop() {
  const row = bar.querySelector(":scope > .dinput.dgrow"), btn = $("#stopRun");
  const running = S.view === "chat" && !!here()?.run;
  if (!row || running === !!btn) return;
  if (!running) return btn?.remove();
  const stopBtn = h(`<button class="dbtn" id="stopRun" aria-label="Stop the reply">${stopFace()}</button>`);
  stopBtn.onclick = () => { stop(); focusAsk(); };
  row.insertBefore(stopBtn, $("#grip"));
}

// ---------- Live mode keys ----------

/// What can be said in live mode, and its keys, in the
/// sheet ⌘/ opens over live mode.
export const LIVE_SAY: Hint[] = [["“send”", "Send now"], ["“stop”", "Send now, or stop Beep speaking"]];
export const LIVE_TALK: Hint[] = [["tap ⌥", "Leave, or stop Beep"], ["hold ⌥", "Talk, or interrupt Beep"], ["esc", "Cancel the reply, or leave"], ["any key", "Leave and type instead"], ["⌥ ⌥", "Over the orb: bring Beep back. In Beep: leave"]];

/// The sections of a key sheet: each title, then its keys and what they do.
export const keyGroups = (sections: readonly (readonly [string, readonly Hint[]])[]) =>
  `<div class="dkeys">${sections.map(([sec, rows]) => `<div class="kg"><div class="dsec">${sec}</div>${rows.map(([k, t]) => `<div class="kr"><span>${t}</span><kbd>${k}</kbd></div>`).join("")}</div>`).join("")}</div>`;

