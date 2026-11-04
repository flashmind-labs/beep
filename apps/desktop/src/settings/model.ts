import { $, bar, esc, ICON, invoke, S } from "../bar/core.ts";
import { hints, refreshHints } from "../bar/hints.ts";
import { cap } from "../chat/tools.ts";

// ---------- Model (admins) ----------

/// How long the model reasons before it answers (ReasoningLevel).
export type Thinking = "off" | "low" | "medium" | "high";

/// A model the caller can pick (ModelChoice).
export interface ModelChoice {
  id: string;
  name: string;
  /// USD per million tokens, when the provider lists it.
  input_price: number | null;
  output_price: number | null;
  sees_images: boolean;
  default: boolean;
}

/// The models the caller can pick from and the one their turns use (Models).
export interface Models {
  /// Empty on servers from before every user could pick.
  choices: ModelChoice[];
  selected: string;
  thinking: Thinking;
  auto: boolean;
  auto_available: boolean;
}

/// Whether agents are on for everyone, and the agents set up (Agents).
interface Agents {
  enabled: boolean;
  agents: { id: string; name: string; when: string; model: string; thinking: Thinking }[];
}

/// The caller's model settings.
export const MODELS: { info: Models | null; sel: number; agents: boolean; list: string[] } = { info: null, sel: 0, agents: false, list: [] };
export const THINKING: Thinking[] = ["off", "low", "medium", "high"];

/// Whether the server offers model choices (every user once it lists them).
export function canPickModel() { return !!MODELS.info?.choices.length; }
export async function loadModels() {
  try {
    MODELS.info = await invoke<Models>("get_models");
  } catch {
    return;
  }
  refreshHints();
  if (!canPickModel()) return;
  try {
    takeAgents(await invoke<Agents>("get_agents"));
  } catch {
    return;
  }
  if (S.view === "model") drawModel();
}
function takeAgents(agents: Agents) {
  MODELS.agents = agents.enabled;
  MODELS.list = agents.agents.map(a => a.name);
}
export function modelName(id: string) { return MODELS.info?.choices.find(c => c.id === id)?.name ?? id.split(/[:/]/).pop()!; }
export function modelPrice(c: ModelChoice) {
  const usd = (n: number) => "$" + (Math.round(n * 100) / 100);
  const price = c.input_price != null && c.output_price != null ? `${usd(c.input_price)} in, ${usd(c.output_price)} out per M tokens` : "No price listed";
  return c.sees_images ? price : price + ", text only";
}
/// The hint row's chip: the model and thinking every turn uses, or Auto.
export function modelChip() {
  const m = MODELS.info;
  if (!m?.choices.length) return "";
  const label = m.auto ? "Auto" : `${modelName(m.selected)} · ${cap(m.thinking)}`;
  return `<button class="dmodel" data-act="model">${esc(label)}<kbd>⌘E</kbd></button>`;
}

export function renderModel() {
  const m = MODELS.info;
  if (!m?.choices.length) return;
  S.view = "model";
  MODELS.sel = Math.max(0, m.choices.findIndex(c => c.id === m.selected));
  drawModel();
}
function drawModel() {
  const m = MODELS.info!, agents = MODELS.agents;
  const autoSub = m.auto ? (agents ? "Jev picks an agent for each prompt" : "Jev picks the thinking for each prompt") : "Off";
  const auto = m.auto_available
    ? `<div class="dtoggle"><span><b>Auto</b><small>${autoSub}</small></span><button class="dswitch" data-act="auto" role="switch" aria-checked="${m.auto}" aria-label="Auto"></button></div>`
    : "";
  const rows = m.choices.map((c, i) => `<button class="drt dmrow" role="option" data-pick="${i}" aria-selected="${i === MODELS.sel}"><span class="tt"><b>${esc(c.name)}</b><span>${esc(modelPrice(c))}</span></span>${c.id === m.selected ? `<span class="dok">${ICON.check}</span>` : ""}</button>`).join("");
  const picker = `<div class="dlist dflush" role="listbox" aria-label="Models">${rows}</div>
      <div class="dfield"><span>Thinking</span><div class="dchips">${THINKING.map(l => `<button class="dchip" data-level="${l}" aria-pressed="${l === m.thinking}">${cap(l)}</button>`).join("")}</div></div>`;
  // Agents apply to everyone, so they sit apart from the caller's own model.
  const agentSec = MODELS.list.length ? `<div class="dsec flat dagents">Agents <span class="dtag">Admin</span></div>
      <div class="dtoggle"><span><b>Agents</b><small>${agents ? "On for everyone" : "Off for everyone"}</small></span><button class="dswitch" data-act="agents" role="switch" aria-checked="${agents}" aria-label="Agents"></button></div>
      <div class="dmuted dagentlist"${agents ? "" : ' style="opacity:.6"'}>${esc(MODELS.list.join(", "))}</div>` : "";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Model</b><small>Your own turns only</small></span></div>
    <div class="dform dmodelform">${auto}
      ${picker}
      ${agentSec}
      <div class="derr" id="modelErr"></div>
    </div>${hints([["↑↓", "move"], [ICON.ret, "use"], ["esc", "back"]])}`;
}
/// Runs a change the server answers with `T`, which `take` stores; an error
/// shows under the form.
async function change<T>(command: string, args: Record<string, unknown>, take: (answer: T) => void) {
  try {
    take(await invoke<T>(command, args));
  } catch (e) {
    const err = $("#modelErr");
    if (err) err.textContent = String(e);
    return;
  }
  if (S.view === "model") drawModel();
}
function changeModel(command: string, args: Record<string, unknown>) {
  return change<Models>(command, args, m => { MODELS.info = m; });
}
const pickModel = () => changeModel("set_model", { id: MODELS.info!.choices[MODELS.sel].id });
export function modelClick(e: MouseEvent) {
  const t = e.target as HTMLElement;
  const pick = t.closest<HTMLElement>("[data-pick]");
  if (pick) { MODELS.sel = +pick.dataset.pick!; return pickModel(); }
  const level = t.closest<HTMLElement>("[data-level]");
  if (level) return changeModel("set_thinking", { level: level.dataset.level });
  if (t.closest('[data-act="auto"]')) return changeModel("set_auto_model", { enabled: !MODELS.info!.auto });
  if (t.closest('[data-act="agents"]')) return change("set_agents", { enabled: !MODELS.agents }, takeAgents);
}
export function modelKeys(e: KeyboardEvent) {
  const m = MODELS.info!;
  if (MODELS.agents && m.auto_available && m.auto) return;
  const n = m.choices.length;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    MODELS.sel = (MODELS.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
    return drawModel();
  }
  if (e.key === "Enter") { e.preventDefault(); return pickModel(); }
}
