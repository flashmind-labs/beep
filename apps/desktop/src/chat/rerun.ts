import { ANSWER, askText, body, follow, keepDown, newTurn, runTurn, scrollDown, startRun } from "./chat.ts";
import { $, COPY_ICON, esc, h, here, ICON, invoke, S, stopping } from "../bar/core.ts";
import type { Turn } from "../bar/core.ts";
import { refreshHints } from "../bar/hints.ts";
import { PLAY_ICON } from "../lists/memories.ts";
import { canPickModel, modelName, modelPrice, MODELS, THINKING } from "../settings/model.ts";
import { cap } from "./tools.ts";

// ---------- Reply actions ----------

/// What the rerun menu reads of `MODELS.info` (`Models`).
interface ModelChoices {
  choices: (Parameters<typeof modelPrice>[0] & { id: string; name: string })[];
}

/// "Stopped" under a reply the user or another device stopped, with Retry,
/// which sends the prompt again, for a prompt sent from this chat.
export function stoppedRow(turn: Turn) {
  turn.querySelector(":scope > .dstopped")?.remove();
  const retry = turn.user ? '<button class="dbtn" data-act="retry">Retry</button>' : "";
  turn.append(h(`<div class="dmuted dstopped">Stopped${retry}</div>`));
}

/// Copy, and for admins Rerun, under a finished reply, then the agent that
/// answered it when there was one.
export function replyActs(turn: Turn) {
  turn.querySelector(".dreplyacts")?.remove();
  const text = turn.querySelector<HTMLElement>(ANSWER)?.innerText.trim();
  const rerun = canPickModel() && turn.user;
  if (!text && !rerun && !turn.agent) return;
  const agent = turn.agent ? `<span class="dagent" title="The agent that answered">${esc(turn.agent)}</span>` : "";
  const row = h(`<div class="dreplyacts">${text ? `<button class="dcopy" data-act="copyReply">${COPY_ICON}Copy</button>` : ""}${rerun ? `<span class="drerun"><button class="dcopy" data-act="rerunMenu" aria-haspopup="menu" aria-expanded="false">${PLAY_ICON}Rerun<span class="dchev">${ICON.chev}</span></button></span>` : ""}${agent}</div>`);
  const replies = turn.querySelector(".dreplies");
  replies ? turn.insertBefore(row, replies) : turn.append(row);
}
export function closeRerunMenu() {
  const menu = $(".dmenu");
  if (!menu) return false;
  $('[data-act="rerunMenu"][aria-expanded="true"]')?.setAttribute("aria-expanded", "false");
  menu.remove();
  return true;
}
export function rerunMenu(button: HTMLElement) {
  if (closeRerunMenu()) return;
  const turn = button.closest<Turn>(".turn")!, m = MODELS.info as ModelChoices | null;
  const rows = m!.choices.map(c => {
    const used = c.id === turn.model;
    return `<button class="mi" role="menuitem" data-rerun="${esc(c.id)}"${used ? ' aria-current="true"' : ""}><b>${esc(c.name)}</b>${used ? `<span class="ok">${ICON.check}</span>` : ""}<small>${used ? "Used for this reply" : esc(modelPrice(c))}</small></button>`;
  }).join("");
  const levels = ["default", ...THINKING].map(l => `<button class="dchip" data-rthink="${l}" aria-pressed="${l === "default"}">${cap(l)}</button>`).join("");
  button.setAttribute("aria-expanded", "true");
  button.closest(".dreplyacts")!.after(h(`<div class="dmenu" role="menu" aria-label="Rerun on">${rows}<div class="think">Thinking<span class="dchips">${levels}</span></div></div>`));
  keepDown();
}
/// Handles a click in the rerun menu: a thinking level marks itself, a
/// model reruns the turn. Says whether the click was the menu's.
export function rerunClick(e: MouseEvent) {
  const target = e.target as Element;
  const level = target.closest("[data-rthink]");
  if (level) {
    level.parentElement!.querySelectorAll(".dchip").forEach(c => c.setAttribute("aria-pressed", String(c === level)));
    return true;
  }
  const pick = target.closest<HTMLElement>("[data-rerun]");
  if (!pick) return false;
  const menu = pick.closest(".dmenu")!, turn = pick.closest<Turn>(".turn")!;
  const chosen = menu.querySelector<HTMLElement>('[data-rthink][aria-pressed="true"]')?.dataset.rthink;
  closeRerunMenu();
  rerun(turn, pick.dataset.rerun!, chosen && chosen !== "default" ? chosen : null);
  return true;
}
/// Drops `turn` and every turn after it, then sends its prompt again on
/// `model`, as the phone's Rerun does.
export async function rerun(turn: Turn, model: string | null, thinking: string | null) {
  const id = S.session, busy = here();
  if (!id || busy?.run || busy?.approval) return;
  const turns = [...body()!.querySelectorAll<Turn>(".turn")];
  const index = turns.filter(t => t.user).indexOf(turn);
  if (index < 0) return;
  const question = askText(turn);
  for (const t of turns.slice(turns.indexOf(turn))) t.remove();
  const fresh = newTurn(question);
  fresh.user = true;
  if (model) {
    fresh.model = model;
    fresh.append(h(`<div class="drerunof">Rerun on ${esc(modelName(model))}${thinking ? " · " + thinking : ""}</div>`));
  }
  startRun(id, fresh, null);
  refreshHints();
  scrollDown();
  await stopping.get(id);
  await runTurn(id, fresh, invoke("rerun_prompt", { session: id, index, pick: { model, thinking }, onEvent: follow(id, fresh) }));
}
