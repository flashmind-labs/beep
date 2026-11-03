// The harness, model and thinking picker, as the start form and a
// project's agent bar both show it: the pill that opens it, the popover
// with its rail, model column and thinking column, and where it goes on
// screen. The caller keeps what is picked and redraws when told.

import { type AgentModels, MODELS_KEPT_MS } from "./agentmodels.ts";
import { effortNote, effortsOf, modelLabel, modelRows, pillDetail, placePopover, railOf } from "./agentpick.ts";
import { agentKind } from "./agents.ts";
import { esc, invoke } from "./core.ts";

const CARET = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
const CHECK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-10"/></svg>';

/// A harness's badge, the window's `cc-ag` tile.
export const badge = (agent: string) => { const k = agentKind(agent); return `<span class="cc-ag ${esc(k.cls)}">${esc(k.ini)}</span>`; };

// ----- Models -----

/// The models each machine's agents offer, by `machine/agent`: `null` when
/// it could not say, `undefined` in `list` while it is asked.
/// `waiting` are told once an answer asked for comes.
const MODELS = new Map<string, { list: AgentModels | null | undefined; at: number; waiting: (() => void)[] }>();

/// The models `agent` offers on `machine` as known now: `undefined` while
/// asked. Asks when not known lately; `loaded` runs once the answer comes.
export function modelsOn(machine: string, agent: string, loaded: () => void): AgentModels | null | undefined {
  const key = `${machine}/${agent}`, known = MODELS.get(key);
  if (known && known.list === undefined) {
    known.waiting.push(loaded);
    return undefined;
  }
  if (known && Date.now() - known.at <= MODELS_KEPT_MS) return known.list;
  const asked = { list: undefined, at: Date.now(), waiting: [loaded] };
  MODELS.set(key, asked);
  invoke<AgentModels | null>("agent_models", { machineId: machine, agent })
    .catch(() => null)
    .then(list => {
      MODELS.set(key, { list, at: Date.now(), waiting: [] });
      for (const tell of asked.waiting) tell();
    });
  return undefined;
}

/// The models known for `agent` on `machine`, without asking.
export const knownModels = (machine: string | null, agent: string | null) => machine && agent ? MODELS.get(`${machine}/${agent}`)?.list : undefined;

// ----- Pill and popover -----

/// What is picked. The caller owns it; the picker changes it.
export interface Picked {
  agent: string;
  /// Empty for the harness's own.
  model: string;
  effort: string | null;
  query: string;
}

export interface PickerOpts {
  picked: Picked;
  /// The harnesses the machine has, and those any machine has.
  offered: readonly string[];
  everywhere: readonly string[];
  /// The machine models are listed on; null when there is none yet.
  machine: { id: string; name: string } | null;
  /// What the model column says without a machine.
  noMachine: string;
  /// The pill the picker opens from, and the area it stays inside.
  anchor: () => HTMLElement | null;
  bounds: () => HTMLElement | null;
  /// After a pick, or once the models are read ("models"); the caller
  /// redraws. A model pick is meant to close the picker.
  changed: (what: "agent" | "model" | "effort" | "models") => void;
}

/// The pill's inside: the harness's badge and name, then the model and
/// thinking picked.
export function pillInner(agent: string, model: string, effort: string | null, machine: string | null) {
  const detail = pillDetail(modelLabel(knownModels(machine, agent), model), agent, effort);
  return `${badge(agent)}${esc(agentKind(agent).name)} <small>${esc(detail)}</small>${CARET}`;
}

/// The picker open now, for placing it again on resize and scroll.
let shown: { el: HTMLElement; o: PickerOpts } | null = null;

/// Draws the picker into `el` and places it by its pill.
export function drawPicker(el: HTMLElement, o: PickerOpts) {
  shown = { el, o };
  const p = o.picked, name = agentKind(p.agent).name, levels = effortsOf(p.agent);
  const rail = railOf(o.offered, o.everywhere).map(r => {
    const k = agentKind(r.agent);
    const why = r.enabled ? k.name : `${k.name} (not on ${o.machine ? o.machine.name : "any machine"})`;
    return `<button type="button" data-agh="${esc(r.agent)}" aria-pressed="${r.agent === p.agent}" title="${esc(why)}"${r.enabled ? "" : " disabled"}>${badge(r.agent)}</button>`;
  }).join("");
  const think = levels.length
    ? [`<button type="button" data-age="" class="${p.effort ? "" : "on"}">Default${p.effort ? "" : CHECK}</button>`,
      ...levels.map(l => `<button type="button" data-age="${l.value}" class="${p.effort === l.value ? "on" : ""}">${esc(l.label)}${p.effort === l.value ? CHECK : ""}</button>`)].join("")
    : `<p class="none">${esc(effortNote(p.agent, name))}</p>`;
  el.innerHTML = `<div class="agc-pick" role="dialog" aria-label="Harness, model and thinking">
      <div class="agc-rail">${rail}</div>
      <div class="agc-list"><input class="agc-search" spellcheck="false" autocomplete="off" placeholder="Search ${esc(name)} models…" value="${esc(p.query)}"><div class="agc-models"></div></div>
      <div class="agc-think"><h5><span>Thinking</span><span>${esc(name)}</span></h5>${think}</div>
    </div>`;
  el.onclick = e => {
    const at = (sel: string) => (e.target as HTMLElement).closest<HTMLElement>(sel);
    const harness = at("[data-agh]"), model = at("[data-agmo]"), effort = at("[data-age]");
    if (harness) { p.agent = harness.dataset.agh!; p.query = ""; p.model = ""; o.changed("agent"); }
    else if (model) { p.model = model.dataset.agmo!; p.query = ""; o.changed("model"); }
    else if (effort) { p.effort = effort.dataset.age || null; o.changed("effort"); }
  };
  const q = el.querySelector<HTMLInputElement>(".agc-search")!;
  q.addEventListener("input", () => { p.query = q.value; drawModels(el, o); });
  q.addEventListener("keydown", e => {
    if (e.key !== "Enter" || e.metaKey) return;
    e.preventDefault();
    const first = el.querySelector<HTMLElement>("[data-agmo]");
    if (first) { p.model = first.dataset.agmo!; p.query = ""; o.changed("model"); }
  });
  drawModels(el, o);
}

/// Focuses the picker's search, when it shows in `el`.
export function focusSearch(el: HTMLElement) {
  el.querySelector<HTMLInputElement>(".agc-search")?.focus();
}

/// Forgets the picker in `el`, once its caller empties it.
export function closedPicker(el: HTMLElement) {
  if (shown?.el === el) shown = null;
}

/// The model column, for what the search holds.
function drawModels(el: HTMLElement, o: PickerOpts) {
  const box = el.querySelector<HTMLElement>(".agc-models"), p = o.picked;
  if (!box) return;
  if (!o.machine) {
    box.innerHTML = `<button type="button" data-agmo="" class="on">Default<small>its own</small></button><p class="none">${esc(o.noMachine)}</p>`;
    return placeOpen();
  }
  const machine = o.machine.id, agent = p.agent;
  const list = modelsOn(machine, agent, () => { if (shown?.el === el && el.isConnected && p.agent === agent) { drawModels(el, o); o.changed("models"); } });
  const { rows, note } = modelRows(list, p.query);
  box.innerHTML = rows.map(r => `<button type="button" data-agmo="${esc(r.value)}" class="${r.value === p.model ? "on" : ""}${r.value && agent === "flsh" ? " mono" : ""}">${esc(r.label)}${r.note ? `<small>${esc(r.note)}</small>` : ""}</button>`).join("")
    + (note ? `<p class="none">${list === undefined ? '<span class="dspin"></span> ' : ""}${esc(note)}</p>` : "");
  placeOpen();
}

/// Puts the open picker above its pill, right edges lined up, whole and
/// inside its bounds: below the pill when it does not fit above, else cut
/// to the roomier side, where its columns scroll.
export function placeOpen() {
  if (!shown || !shown.el.isConnected) { shown = null; return; }
  const pick = shown.el.querySelector<HTMLElement>(".agc-pick"), pill = shown.o.anchor(), area = shown.o.bounds();
  if (!pick || !pill || !area) return;
  pick.style.removeProperty("--pick-h");
  pick.style.width = "";
  const pane = area.getBoundingClientRect(), a = pill.getBoundingClientRect();
  const top = Math.max(0, pane.top), left = Math.max(0, pane.left);
  const view = { width: Math.min(pane.right, innerWidth) - left, height: Math.min(pane.bottom, innerHeight) - top };
  const at = placePopover({ top: a.top - top, left: a.left - left, width: a.width, height: a.height }, { width: pick.offsetWidth, height: pick.offsetHeight }, view);
  pick.style.top = `${at.top + top}px`;
  pick.style.left = `${at.left + left}px`;
  pick.style.width = `${at.width}px`;
  pick.style.setProperty("--pick-h", at.maxHeight === null ? "" : `${at.maxHeight}px`);
}

addEventListener("resize", placeOpen);
addEventListener("scroll", placeOpen, { capture: true, passive: true });
