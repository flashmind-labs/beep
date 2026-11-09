import { createElement, type ReactElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { AgentPicker } from "./agentpicker-view.tsx";
// The harness, model and thinking picker, as the start form and a project's
// agent bar both show it: the pill that opens it, the popover with its rail,
// model column and thinking column, and where it goes on screen.

import { type AgentModels, MODELS_KEPT_MS } from "./agentmodels.ts";
import { modelLabel, modelRows, pillDetail, placePopover } from "./agentpick.ts";
import { harnessTile } from "../ui/harnessicon.ts";
import { esc } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";

const CARET = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';

// ----- Models -----

/// The models each machine's agents offer, by `machine/agent`: `null` when it
/// could not say, `undefined` in `list` until the first answer.
const MODELS = new Map<string, ModelsEntry>();
interface ModelsEntry { list: AgentModels | null | undefined; at: number; asking: boolean; waiting: (() => void)[] }

/// Where the lists are kept between launches; both windows share it.
const MODELS_STORE = "beep.agentModels.v1";
/// Lists older than this are not shown again after a restart.
const MODELS_STORE_MAX_MS = 14 * 24 * 60 * 60 * 1000;

function restoreModels() {
  let kept: Record<string, { list: AgentModels; at: number }> = {};
  try { kept = JSON.parse(localStorage.getItem(MODELS_STORE) || "{}") || {}; } catch { return; }
  const now = Date.now();

  for (const [key, v] of Object.entries(kept)) {
    if (v && v.list && typeof v.at === "number" && now - v.at < MODELS_STORE_MAX_MS) {
      // Shown at once, and asked again the first time it is wanted.
      MODELS.set(key, { list: v.list, at: 0, asking: false, waiting: [] });
    }
  }
}

function storeModels() {
  const out: Record<string, { list: AgentModels; at: number }> = {};
  for (const [key, v] of MODELS) if (v.list) out[key] = { list: v.list, at: v.at || Date.now() };
  try { localStorage.setItem(MODELS_STORE, JSON.stringify(out)); } catch { /* kept for this launch only */ }
}

restoreModels();

/// Asks `machine` for `agent`'s models unless a request is out; `loaded`
/// runs once the answer comes, if it changed what is shown.
function askModels(key: string, machine: string, agent: string, loaded: (() => void) | null) {
  let entry = MODELS.get(key);
  if (!entry) MODELS.set(key, entry = { list: undefined, at: 0, asking: false, waiting: [] });
  if (loaded) entry.waiting.push(loaded);
  if (entry.asking) return;
  entry.asking = true;
  invoke<AgentModels | null>("agent_models", { machineId: machine, agent })
    .catch(() => null)
    .then(list => {
      const e = MODELS.get(key)!;
      const waiting = e.waiting;
      // A failed refresh keeps the list shown.
      const next = list ?? (e.list === undefined ? null : e.list);
      const changed = JSON.stringify(next) !== JSON.stringify(e.list);
      MODELS.set(key, { list: next, at: Date.now(), asking: false, waiting: [] });
      if (list) storeModels();
      if (changed) for (const tell of waiting) tell();
    });
}

/// Whether `e` holds an answer read lately.
const fresh = (e: ModelsEntry | undefined) => !!e && e.list !== undefined && Date.now() - e.at <= MODELS_KEPT_MS;

/// The models `agent` offers on `machine` as known now: `undefined` only before
/// the first answer.
export function modelsOn(machine: string, agent: string, loaded: () => void): AgentModels | null | undefined {
  const key = `${machine}/${agent}`, known = MODELS.get(key);
  if (fresh(known)) return known!.list;
  askModels(key, machine, agent, loaded);
  return known?.list;
}

/// Reads the models of every harness on `hosts` ahead of the picker, such
/// as when a machine connects; lists read lately are left alone.
export function preloadModels(hosts: readonly { machine_id: string; agents: readonly string[] }[]) {
  for (const h of hosts) {
    for (const agent of h.agents) {
      const key = `${h.machine_id}/${agent}`;
      if (!fresh(MODELS.get(key))) askModels(key, h.machine_id, agent, null);
    }
  }
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
  render?: (view: ReactElement | null) => void;
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

/// The pill's inside: the harness badge, thinking level and model.
export function pillInner(agent: string, model: string, effort: string | null, machine: string | null) {
  const detail = pillDetail(modelLabel(knownModels(machine, agent), model), agent, effort);
  return `${harnessTile(agent)}${esc(detail)}${CARET}`;
}

/// The picker open now, for placing it again on resize and scroll.
let shown: { el: HTMLElement; o: PickerOpts } | null = null;
let attachmentWatch: MutationObserver | null = null;
let watchedElement: HTMLElement | null = null;

const roots = new WeakMap<HTMLElement, Root>();
const renderers = new WeakMap<HTMLElement, (view: ReactElement | null) => void>();

/// Draws the picker into `el` and places it by its pill.
export function drawPicker(el: HTMLElement, o: PickerOpts) {
  if (shown?.el !== el) {
    attachmentWatch?.disconnect();
    watchedElement = el;
    attachmentWatch = new MutationObserver(() => { if (!el.isConnected) closedPicker(el); });
    attachmentWatch.observe(document.documentElement, { childList: true, subtree: true });
  }
  shown = { el, o };
  const p = o.picked;
  if (o.render) renderers.set(el, o.render);
  else if (!roots.has(el)) roots.set(el, createRoot(el));
  const agent = p.agent;
  const list = o.machine ? modelsOn(o.machine.id, agent, () => {
    if (shown?.el !== el || !el.isConnected || shown.o.picked.agent !== agent) return;
    const current = shown.o;
    drawPicker(el, current);
    current.changed("models");
  }) : null;
  const data = o.machine ? modelRows(list, p.query) : { rows: [{ value: "", label: "Default", note: "its own" }], note: o.noMachine };
  const view = createElement(AgentPicker, {
    picked: p, offered: o.offered, everywhere: o.everywhere, machineName: o.machine?.name ?? null, machineId: o.machine?.id ?? null,
    rows: data.rows, note: data.note, loading: !!o.machine && list === undefined,
    onQuery: (query: string) => { p.query = query; drawPicker(el, o); },
    onAgent: (next: string) => { p.agent = next; p.query = ""; p.model = ""; o.changed("agent"); },
    onModel: (next: string) => { p.model = next; p.query = ""; o.changed("model"); },
    onEffort: (next: string | null) => { p.effort = next; o.changed("effort"); },
  });
  if (o.render) o.render(view);
  else flushSync(() => roots.get(el)?.render(view));
  placeOpen();
}

/// Focuses the picker's search, when it shows in `el`.
export function focusSearch(el: HTMLElement) {
  el.querySelector<HTMLInputElement>(".agc-search")?.focus();
}

/// Unmounts the picker and releases its React root.
export function closedPicker(el: HTMLElement) {
  if (shown?.el === el) shown = null;
  if (watchedElement === el) {
    attachmentWatch?.disconnect();
    attachmentWatch = null;
    watchedElement = null;
  }
  const render = renderers.get(el);
  if (render) { renderers.delete(el); render(null); }
  const root = roots.get(el);
  if (!root) return;
  root.unmount();
  roots.delete(el);
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
