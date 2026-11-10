import { turnState } from "./turn-state.ts";
import { dropFrom, setView, turnsOf, viewOf, type Turn } from "./transcript.ts";
import { askText, newTurn } from "./turns.ts";
import { body, keepDown, scrollDown } from "./chat.ts";
import { follow, runTurn, startRun } from "./runs.ts";
import { here, S, stopping } from "../bar/core.ts";
import { refreshHints } from "../bar/hints.ts";
import { canPickModel, modelName, modelPrice, MODELS, THINKING } from "../settings/model.ts";
import { cap } from "./tools.ts";
import { invoke } from "../ui/tauri.ts";

// ---------- Reply actions ----------

/// What the rerun menu reads of `MODELS.info` (`Models`).
interface ModelChoices {
  choices: (Parameters<typeof modelPrice>[0] & { id: string; name: string })[];
}

/// "Stopped" under a reply the user or another device stopped, or `said`
/// for one a server restart cut off, with Retry, which sends the prompt
/// again, for a prompt sent from this chat.
export function stoppedRow(turn: Turn, said = "Stopped") {
  setView(turn, { stopped: { message: said, retry: turnState(turn).user ? () => void rerun(turn, null, null) : null } });
}

/// Copy, and for admins Rerun, under a finished reply, then the agent that
/// answered it when there was one.
export function replyActs(turn: Turn) {
  const copy = !!replyText(viewOf(turn).reply);
  const rerun = !!(canPickModel() && turnState(turn).user), agent = turnState(turn).agent ?? null;
  setView(turn, { actions: copy || rerun || agent ? { copy, rerun: rerun ? () => toggleRerunMenu(turn) : null, agent } : null });
}

/// The words of a reply's markup, as the reader sees them.
function replyText(html: string | null) {
  if (!html) return "";
  const markup = document.createElement("template");
  markup.innerHTML = html;
  return markup.content.textContent?.trim() ?? "";
}

/// Closes the open chat's rerun menu; false when none was open.
export function closeRerunMenu() {
  const b = body(), open = b ? turnsOf(b).filter(turn => viewOf(turn).menu) : [];
  for (const turn of open) setView(turn, { menu: null });
  return open.length > 0;
}

function toggleRerunMenu(turn: Turn) {
  if (viewOf(turn).menu) { closeRerunMenu(); return; }
  closeRerunMenu();
  const m = MODELS.info as ModelChoices | null;
  if (!m) return;
  const used = turnState(turn).model;
  setView(turn, { menu: {
    choices: m.choices.map(choice => ({ id: choice.id, name: choice.name, used: choice.id === used, detail: choice.id === used ? "Used for this reply" : modelPrice(choice) })),
    levels: ["default", ...THINKING].map(id => ({ id, label: cap(id) })),
    run: (model, thinking) => { closeRerunMenu(); void rerun(turn, model, thinking); },
  } });
  keepDown();
}

/// `model`, as the phone's Rerun does.
export async function rerun(turn: Turn, model: string | null, thinking: string | null) {
  const id = S.session, busy = here();
  if (!id || busy?.run || busy?.approval) return;
  const index = turnsOf(turn.body).filter(t => turnState(t).user).indexOf(turn);
  if (index < 0) return;
  const question = askText(turn);
  dropFrom(turn);
  const fresh = newTurn(question);
  turnState(fresh).user = true;

  if (model) {
    turnState(fresh).model = model;
    setView(fresh, { rerunOf: `Rerun on ${modelName(model)}${thinking ? " · " + thinking : ""}` });
  }

  startRun(id, fresh, null);
  refreshHints();
  scrollDown();
  await stopping.get(id);
  await runTurn(id, fresh, invoke("rerun_prompt", { session: id, index, pick: { model, thinking }, onEvent: follow(id, fresh) }));
}
