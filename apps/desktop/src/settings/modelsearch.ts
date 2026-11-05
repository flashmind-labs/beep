// The model page of a chat kept on this Mac: a search over OpenRouter's catalog, the catalog's
// feature toggles, and the upstream provider preferred for the picked model. model.ts keeps the
// server's page and hands this one the models it already holds.
import { $, $$, bar } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
import { cap } from "../chat/tools.ts";
import { esc } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { lazyRows, resetRows } from "../ui/lazyrows.ts";
import { invoke } from "../ui/tauri.ts";
import {
  type CatalogModel, type Feature, FEATURES, type ProviderOption, type ProviderPref,
  detailLine, filterModels, providerFor, stepSel, toggleFeature,
} from "./modelsearch-state.ts";

/// What model.ts shares: the chip's models, which it keeps, and the levels of thinking.
export interface LocalHost {
  selected(): string;
  thinking(): string;
  levels: readonly string[];
  /// Runs a chip command (`set_model`, `set_thinking`) and keeps its answer.
  change(command: string, args: Record<string, unknown>): Promise<boolean>;
}

/// The catalog and the page's state (assistant `Picker`).
interface Picker {
  models: CatalogModel[];
  provider: ProviderPref | null;
}

interface Local {
  models: CatalogModel[];
  shown: CatalogModel[];
  query: string;
  active: Feature[];
  sel: number;
  pref: ProviderPref | null;
  /// The providers of the picked model, once asked for.
  providers: ProviderOption[] | null;
  providersFor: string;
  loading: boolean;
}

const LOCAL: Local = { models: [], shown: [], query: "", active: ["chat"], sel: 0, pref: null, providers: null, providersFor: "", loading: false };
let host: LocalHost;

const err = (text: string) => { const e = $("#modelErr"); if (e) e.textContent = text; };

function htmlNodes(html: string) {
  const t = document.createElement("template");
  t.innerHTML = html;
  return [...t.content.childNodes];
}

/// Opens the page. The catalog loads in the background; the rows draw when it arrives.
export function renderLocalModel(h: LocalHost) {
  host = h;
  Object.assign(LOCAL, { query: "", active: ["chat"], sel: 0, providers: null, providersFor: "" });
  resetRows("models");
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Model</b><small>Your own turns only</small></span></div>
    <div class="dform dmodelform">
      <div class="dfield"><div class="f"><input id="msq" placeholder="Search models" autocomplete="off" spellcheck="false" style="flex:1;min-width:0;border:0;background:transparent;color:inherit;font:inherit;outline:0"></div></div>
      <div class="dchips" id="mschips"></div>
      <div class="dlist dflush" id="mslist" role="listbox" aria-label="Models" style="max-height:210px;overflow:auto"></div>
      <div class="dfield"><span>Provider</span><div class="dchips" id="msprov"></div></div>
      <div class="dfield"><span>Thinking</span><div class="dchips" id="msthink"></div></div>
      <div class="derr" id="modelErr"></div>
    </div>${hints([["↑↓", "move"], [ICON.ret, "use"], ["esc", "back"]])}`;
  const find = $$<HTMLInputElement>("#msq");
  find.focus();
  find.addEventListener("input", () => { LOCAL.query = find.value; LOCAL.sel = 0; drawRows(); });
  drawChips();
  drawThinking();
  drawProvider();
  drawRows();
  void load();
}

async function load() {
  try {
    const picker = await invoke<Picker>("local_picker", { feature: "chat" });
    LOCAL.models = picker.models;
    LOCAL.pref = picker.provider;
  } catch (e) {
    err(String(e));
    return;
  }
  const at = LOCAL.models.findIndex(m => m.id === host.selected());
  LOCAL.sel = Math.max(0, at);
  drawRows();
  drawProvider();
}

/// The rows of the models that match, a screenful at a time.
function drawRows() {
  const list = $("#mslist");
  if (!list) return;
  LOCAL.shown = filterModels(LOCAL.models, LOCAL.query, LOCAL.active);
  LOCAL.sel = Math.min(LOCAL.sel, Math.max(0, LOCAL.shown.length - 1));
  const picked = host.selected();
  const make = (from: number, to: number) => htmlNodes(LOCAL.shown.slice(from, to).map((m, k) => row(m, from + k, picked)).join(""));
  const nodes = LOCAL.shown.length
    ? lazyRows("models", `${LOCAL.query}|${LOCAL.active.join()}`, LOCAL.shown.length, make, LOCAL.sel, 30)
    : htmlNodes(`<div class="dmuted dpad">${LOCAL.models.length ? "No matching models" : "Loading models"}</div>`);
  list.replaceChildren(...nodes);
  list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
}

function row(m: CatalogModel, i: number, picked: string) {
  const marks = `${m.tools ? '<span class="dtag">Tools</span>' : ""}${m.images ? '<span class="dtag">Pictures</span>' : ""}`;
  return `<button class="drt dmrow" role="option" data-pick="${i}" aria-selected="${i === LOCAL.sel}"><span class="tt"><b>${esc(m.name)}</b><span>${esc(detailLine(m))}</span></span>${marks}${m.id === picked ? `<span class="dok">${ICON.check}</span>` : ""}</button>`;
}

/// Moves the pick: only the two rows concerned change, unless the new one is not drawn yet.
function moveSel(to: number) {
  const list = $("#mslist");
  const was = LOCAL.sel;
  LOCAL.sel = to;
  const next = list?.querySelector<HTMLElement>(`[data-pick="${to}"]`);
  if (!list || !next) return drawRows();
  list.querySelector(`[data-pick="${was}"]`)?.setAttribute("aria-selected", "false");
  next.setAttribute("aria-selected", "true");
  next.scrollIntoView({ block: "nearest" });
}

function drawChips() {
  $$("#mschips").innerHTML = FEATURES.map(f => `<button class="dchip" data-feature="${f}" aria-pressed="${LOCAL.active.includes(f)}"${f === "chat" ? " disabled" : ""}>${f}</button>`).join("");
}

function drawThinking() {
  $$("#msthink").innerHTML = host.levels.map(l => `<button class="dchip" data-level="${l}" aria-pressed="${l === host.thinking()}">${cap(l)}</button>`).join("");
}

/// Any, the saved provider and, once asked for, the model's providers.
function drawProvider() {
  const model = host.selected();
  const tag = providerFor(LOCAL.pref, model);
  const chip = (value: string, label: string, on: boolean) => `<button class="dchip" data-prov="${esc(value)}" aria-pressed="${on}">${esc(label)}</button>`;
  const known = LOCAL.providersFor === model ? LOCAL.providers : null;
  const names = known ?? (tag ? [{ tag, name: tag }] : []);
  $$("#msprov").innerHTML = chip("", "Any", !tag) + names.map(p => chip(p.tag, p.name, p.tag === tag)).join("")
    + (known ? "" : `<button class="dchip" data-act="provload"${LOCAL.loading ? " disabled" : ""}>${LOCAL.loading ? "Loading" : "Choose"}</button>`);
}

async function loadProviders() {
  const model = host.selected();
  LOCAL.loading = true;
  err("");
  drawProvider();
  try {
    LOCAL.providers = await invoke<ProviderOption[]>("local_providers", { model });
    LOCAL.providersFor = model;
  } catch (e) {
    err(`Could not list providers: ${String(e)}`);
  }
  LOCAL.loading = false;
  drawProvider();
}

async function pickProvider(tag: string) {
  const model = host.selected();
  try {
    LOCAL.pref = await invoke<ProviderPref | null>("set_local_provider", { model, tag: tag || null });
  } catch (e) {
    err(String(e));
    return;
  }
  drawProvider();
}

async function pickModel() {
  const m = LOCAL.shown[LOCAL.sel];
  if (!m) return;
  if (!await host.change("set_model", { id: m.id })) return;
  // A provider belongs to one model, and the saved one went with the old pick.
  if (LOCAL.pref && LOCAL.pref.model !== m.id) LOCAL.pref = null;
  LOCAL.providers = null;
  drawRows();
  drawProvider();
}

/// A change of the chip's models came back (a model, or the thinking).
export function localChanged() {
  if (!$("#mslist")) return;
  drawRows();
  drawThinking();
  drawProvider();
}

export function localClick(e: MouseEvent) {
  const t = e.target as HTMLElement;
  const pick = t.closest<HTMLElement>("[data-pick]");
  if (pick) { LOCAL.sel = +pick.dataset.pick!; return pickModel(); }
  const feature = t.closest<HTMLElement>("[data-feature]");
  if (feature) {
    LOCAL.active = toggleFeature(LOCAL.active, feature.dataset.feature as Feature);
    LOCAL.sel = 0;
    drawChips();
    drawRows();
    return $("#msq")?.focus();
  }
  const level = t.closest<HTMLElement>("[data-level]");
  if (level) return host.change("set_thinking", { level: level.dataset.level });
  if (t.closest('[data-act="provload"]')) return loadProviders();
  const prov = t.closest<HTMLElement>("[data-prov]");
  if (prov) return pickProvider(prov.dataset.prov!);
}

export function localKeys(e: KeyboardEvent) {
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    return moveSel(stepSel(LOCAL.sel, LOCAL.shown.length, e.key));
  }
  if (e.key === "Enter") { e.preventDefault(); return pickModel(); }
}
