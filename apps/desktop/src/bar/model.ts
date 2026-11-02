import { $, bar, esc, ICON, invoke, S } from "./core.ts";
import { hints, refreshHints } from "./hints.ts";
import { cap } from "./tools.ts";

// ---------- Model (admins) ----------

/// The caller's model settings. `choices` is empty unless they are an
/// admin, and then the bar shows no chip, no model view and no rerun.
/// `agents` is whether an admin turned agents on for everyone.
/// `list` names the agents set up, General first; empty when there are none.
export const MODELS = { info: null, sel: 0, agents: false, list: [] };
export const THINKING = ["off", "low", "medium", "high"];

/// Whether the server offers model choices (every user once it lists them).
export function canPickModel() { return !!MODELS.info?.choices.length; }
export async function loadModels() {
  try {
    MODELS.info = await invoke("get_models");
  } catch {
    return;
  }
  refreshHints();
  if (!canPickModel()) return;
  try {
    const agents = await invoke("get_agents");
    MODELS.agents = agents.enabled;
    MODELS.list = agents.agents.map(a => a.name);
  } catch {
    return;
  }
  if (S.view === "model") drawModel();
}
export function modelName(id) { return MODELS.info?.choices.find(c => c.id === id)?.name ?? id.split(/[:/]/).pop(); }
export function modelPrice(c) {
  const usd = n => "$" + (Math.round(n * 100) / 100);
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
export function drawModel() {
  const m = MODELS.info, agents = MODELS.agents;
  // Auto picks only the thinking: the model below always applies, and with
  // agents on the agent Jev picks brings its prompt and thinking.
  const autoSub = m.auto
    ? (agents ? "Jev picks an agent, which brings its prompt and thinking, on the model below" : "Jev picks the thinking for each prompt, on the model below")
    : (agents ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below");
  const auto = m.auto_available
    ? `<div class="dtoggle"><span><b>Auto</b><small>${autoSub}</small></span><button class="dswitch" data-act="auto" role="switch" aria-checked="${m.auto}" aria-label="Auto"></button></div>`
    : "";
  const rows = m.choices.map((c, i) => `<button class="drt dmrow" role="option" data-pick="${i}" aria-selected="${i === MODELS.sel}"><span class="tt"><b>${esc(c.name)}</b><span>${esc(modelPrice(c))}</span></span>${c.id === m.selected ? `<span class="dok">${ICON.check}</span>` : ""}</button>`).join("");
  const picker = `<div class="dlist" style="padding:0" role="listbox" aria-label="Models">${rows}</div>
      <div class="dfield"><span>Thinking</span><div class="dchips">${THINKING.map(l => `<button class="dchip" data-level="${l}" aria-pressed="${l === m.thinking}">${cap(l)}</button>`).join("")}</div></div>`;
  // Agents apply to everyone, so they sit apart from the caller's own model.
  const agentSec = MODELS.list.length ? `<div class="dsec flat dagents">Agents <span class="dtag">Admin</span></div>
      <div class="dtoggle"><span><b>Agents</b><small>${agents ? "On for everyone: Jev picks an agent for each prompt" : "Off: everyone uses the general assistant"}</small></span><button class="dswitch" data-act="agents" role="switch" aria-checked="${agents}" aria-label="Agents"></button></div>
      <div class="dmuted dagentlist"${agents ? "" : ' style="opacity:.6"'}>${esc(MODELS.list.join(", "))}</div>` : "";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Model</b><small>${agents ? "Your own turns only." : "Your own turns only. Everyone else stays on the default."}</small></span></div>
    <div class="dform dmodelform" style="padding-top:12px">${auto}
      ${picker}
      ${agentSec}
      <div class="derr" id="modelErr"></div>
    </div>${hints([["↑↓", "move"], [ICON.ret, "use"], ["esc", "back"]])}`;
}
export async function changeModel(command, args) {
  try {
    MODELS.info = await invoke(command, args);
  } catch (e) {
    const err = $("#modelErr");
    if (err) err.textContent = String(e);
    return;
  }
  if (S.view === "model") drawModel();
}
export function modelClick(e) {
  const pick = e.target.closest("[data-pick]");
  if (pick) { MODELS.sel = +pick.dataset.pick; return changeModel("set_model", { id: MODELS.info.choices[MODELS.sel].id }); }
  const level = e.target.closest("[data-level]");
  if (level) return changeModel("set_thinking", { level: level.dataset.level });
  if (e.target.closest('[data-act="auto"]')) return changeModel("set_auto_model", { enabled: !MODELS.info.auto });
  if (e.target.closest('[data-act="agents"]')) return setAgents(!MODELS.agents);
}
export async function setAgents(enabled) {
  try {
    const agents = await invoke("set_agents", { enabled });
    MODELS.agents = agents.enabled;
    MODELS.list = agents.agents.map(a => a.name);
  } catch (e) {
    const err = $("#modelErr");
    if (err) err.textContent = String(e);
    return;
  }
  if (S.view === "model") drawModel();
}
export function modelKeys(e) {
  if (MODELS.agents && MODELS.info.auto_available && MODELS.info.auto) return;
  const n = MODELS.info.choices.length;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    MODELS.sel = (MODELS.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
    return drawModel();
  }
  if (e.key === "Enter") { e.preventDefault(); return changeModel("set_model", { id: MODELS.info.choices[MODELS.sel].id }); }
}

