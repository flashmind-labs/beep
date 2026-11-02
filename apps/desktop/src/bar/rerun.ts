import { body, failed, finish, follow, keepDown, newTurn, reply, scrollDown } from "./chat.ts";
import { $, COPY_ICON, entry, esc, h, here, ICON, invoke, S, stopping } from "./core.ts";
import { refreshHints } from "./hints.ts";
import { PLAY_ICON } from "./memories.ts";
import { canPickModel, modelName, modelPrice, MODELS, THINKING } from "./model.ts";
import { cap } from "./tools.ts";

// ---------- Reply actions ----------

/// Copy, and for admins Rerun, under a finished reply, then the agent that
/// answered it when there was one.
export function replyActs(turn) {
  turn.querySelector(".dreplyacts")?.remove();
  const text = turn.querySelector(".reply")?.innerText.trim();
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
export function rerunMenu(button) {
  if (closeRerunMenu()) return;
  const turn = button.closest(".turn"), m = MODELS.info;
  const rows = m.choices.map(c => {
    const used = c.id === turn.model;
    return `<button class="mi" role="menuitem" data-rerun="${esc(c.id)}"${used ? ' aria-current="true"' : ""}><b>${esc(c.name)}</b>${used ? `<span class="ok">${ICON.check}</span>` : ""}<small>${used ? "Used for this reply" : esc(modelPrice(c))}</small></button>`;
  }).join("");
  const levels = ["default", ...THINKING].map(l => `<button class="dchip" data-rthink="${l}" aria-pressed="${l === "default"}">${cap(l)}</button>`).join("");
  button.setAttribute("aria-expanded", "true");
  button.closest(".dreplyacts").after(h(`<div class="dmenu" role="menu" aria-label="Rerun on">${rows}<div class="think">Thinking<span class="dchips">${levels}</span></div></div>`));
  keepDown();
}
export function rerunClick(e) {
  const level = e.target.closest("[data-rthink]");
  if (level) {
    level.parentElement.querySelectorAll(".dchip").forEach(c => c.setAttribute("aria-pressed", c === level));
    return true;
  }
  const pick = e.target.closest("[data-rerun]");
  if (!pick) return false;
  const menu = pick.closest(".dmenu"), turn = pick.closest(".turn");
  const level2 = menu.querySelector('[data-rthink][aria-pressed="true"]')?.dataset.rthink;
  closeRerunMenu();
  rerun(turn, pick.dataset.rerun, level2 && level2 !== "default" ? level2 : null);
  return true;
}
/// Drops `turn` and every turn after it, then sends its prompt again on
/// `model`, as the phone's Rerun does.
export async function rerun(turn, model, thinking) {
  const id = S.session, busy = here();
  if (!id || busy?.run || busy?.approval) return;
  const turns = [...body().querySelectorAll(".turn")];
  const index = turns.filter(t => t.user).indexOf(turn);
  if (index < 0) return;
  const question = turn.querySelector(".ask")?.textContent ?? "";
  for (const t of turns.slice(turns.indexOf(turn))) t.remove();
  const fresh = newTurn(question);
  fresh.user = true;
  fresh.model = model;
  fresh.append(h(`<div class="drerunof">Rerun on ${esc(modelName(model))}${thinking ? " · " + thinking : ""}</div>`));
  reply(fresh).innerHTML = '<span class="caret"></span>';
  const e = entry(id);
  e.body = body();
  e.run = { id: null, turn: fresh };
  refreshHints();
  scrollDown();
  await stopping.get(id);
  try {
    await invoke("rerun_prompt", { session: id, index, pick: { model, thinking }, onEvent: follow(id, fresh) });
  } catch (err) {
    if (!fresh.stopped) failed(fresh, String(err));
  }
  fresh.started?.(null);
  if (!fresh.done) finish(id, fresh);
}

