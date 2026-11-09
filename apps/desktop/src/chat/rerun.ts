import { turnState } from "./turn-state.ts";
import { conversation } from "./conversation.ts";
import { ANSWER, askText, body, owningChatBody, follow, keepDown, newTurn, runTurn, scrollDown, startRun } from "./chat.ts";
import { discardChatActions, discardChatNotice, discardChatRerunMenu, renderChatActions, renderChatNotice, renderChatRerunMenu } from "./work-view.tsx";
import { $, here, S, stopping } from "../bar/core.ts";
import { refreshHints } from "../bar/hints.ts";
import { canPickModel, modelName, modelPrice, MODELS, THINKING } from "../settings/model.ts";
import { cap } from "./tools.ts";
import { invoke } from "../ui/tauri.ts";
import type { Turn } from "../ui/turn.ts";

// ---------- Reply actions ----------

/// What the rerun menu reads of `MODELS.info` (`Models`).
interface ModelChoices {
  choices: (Parameters<typeof modelPrice>[0] & { id: string; name: string })[];
}

/// "Stopped" under a reply the user or another device stopped, or `said`
/// for one a server restart cut off, with Retry, which sends the prompt
/// again, for a prompt sent from this chat.
export function stoppedRow(turn: Turn, said = "Stopped") {
  const old = turn.querySelector<HTMLElement>(":scope > .dstopped");
  if (old) { discardChatNotice(turn, old); old.remove(); }
  const owner = owningChatBody(turn);
  if (!owner) return;
  const box = document.createElement("div");
  box.className = "dmuted dstopped";
  renderChatNotice(owner, turn, box, { kind: "stopped", message: said, retry: !!turnState(turn).user });
  turn.append(box);
}

/// Copy, and for admins Rerun, under a finished reply, then the agent that
/// answered it when there was one.
export function replyActs(turn: Turn) {
  const text = !!turn.querySelector<HTMLElement>(ANSWER)?.innerText.trim();
  const rerun = !!(canPickModel() && turnState(turn).user), agent = turnState(turn).agent ?? null;
  let row = turn.querySelector<HTMLElement>(".dreplyacts");
  if (!text && !rerun && !agent) { discardChatActions(turn); row?.remove(); return; }
  if (!row) {
    row = document.createElement("div"); row.className = "dreplyacts";
    const replies = turn.querySelector(".dreplies");
    replies ? turn.insertBefore(row, replies) : turn.append(row);
  }
  const owner = owningChatBody(turn);
  if (owner) renderChatActions(owner, turn, row, { copy: text, rerun, agent });
}
export function closeRerunMenu() {
  const menu = $(".dmenu");
  if (!menu) return false;
  $('[data-act="rerunMenu"][aria-expanded="true"]')?.setAttribute("aria-expanded", "false");
  const turn = menu.closest<Turn>(".turn");
  if (turn) discardChatRerunMenu(turn);
  menu.remove();
  return true;
}
export function rerunMenu(button: HTMLElement) {
  if (closeRerunMenu()) return;
  const turn = button.closest<Turn>(".turn")!, m = MODELS.info as ModelChoices | null;
  const owner = owningChatBody(turn);
  if (!owner || !m) return;
  const menu = document.createElement("div");
  menu.className = "dmenu"; menu.setAttribute("role", "menu"); menu.setAttribute("aria-label", "Rerun on");
  button.setAttribute("aria-expanded", "true");
  button.closest(".dreplyacts")?.after(menu);
  renderChatRerunMenu(owner, turn, menu, {
    choices: m.choices.map(choice => ({ id: choice.id, name: choice.name, used: choice.id === turnState(turn).model, detail: choice.id === turnState(turn).model ? "Used for this reply" : modelPrice(choice) })),
    levels: ["default", ...THINKING].map(id => ({ id, label: cap(id) })),
    run: (model, thinking) => { closeRerunMenu(); void rerun(turn, model, thinking); },
  });
  keepDown();
}
/// `model`, as the phone's Rerun does.
export async function rerun(turn: Turn, model: string | null, thinking: string | null) {
  const id = S.session, busy = here();
  if (!id || busy?.run || busy?.approval) return;
  const current = conversation(body()!), turns = current.turns;
  const index = turns.filter(t => turnState(t).user).indexOf(turn);
  if (index < 0) return;
  const question = askText(turn);
  current.dropFrom(turn);
  const fresh = newTurn(question);
  turnState(fresh).user = true;

  if (model) {
    turnState(fresh).model = model;
    const owner = owningChatBody(fresh);
    if (owner) {
      const box = document.createElement("div");
      box.className = "drerunof";
      renderChatNotice(owner, fresh, box, { kind: "label", message: `Rerun on ${modelName(model)}${thinking ? " · " + thinking : ""}` });
      fresh.append(box);
    }
  }

  startRun(id, fresh, null);
  refreshHints();
  scrollDown();
  await stopping.get(id);
  await runTurn(id, fresh, invoke("rerun_prompt", { session: id, index, pick: { model, thinking }, onEvent: follow(id, fresh) }));
}
