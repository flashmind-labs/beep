// The Assistant page's Dictation section: hold Option and speak, and the words are typed where the
// cursor is. The user picks a provider with a saved key and a model; Rust keeps the pick and the
// keys. `dictation()` returns the one element the page shows; it loads again each time it is asked.
import { el as h } from "../ui/dom.ts";
import { dbtn as button, secHead } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import {
  DICTATION_NOTE, NO_KEYS, active, filterModels, shownModel,
  type DictationProvider, type DictationView, type ModelChoice,
} from "./dictation-state.ts";

const section = h("div", "");
const state = {
  view: undefined as DictationView | null | undefined,
  models: undefined as ModelChoice[] | null | undefined,
  query: "",
  error: "",
};

async function load() {
  state.view = await invoke<DictationView>("dictation_get").catch(e => { console.warn("reading dictation:", e); return null; });
  state.error = "";
  draw();
  if (state.view && active(state.view)?.provider === "openrouter" && !state.models) await loadModels();
}

async function loadModels() {
  state.models = await invoke<ModelChoice[]>("dictation_models").catch(e => { state.error = String(e); return null; });
  draw();
}

async function save(provider: DictationProvider | null, model: string) {
  try {
    await invoke("dictation_set", { provider, model });
    state.error = "";
  } catch (e) {
    state.error = String(e);
  }
  await load();
}

function providerRow(view: DictationView) {
  const on = active(view)?.provider ?? null;
  const tab = (label: string, picked: boolean, pick: () => void) => {
    const b = button(label, picked ? "pri small" : "quiet small", pick);
    b.setAttribute("aria-pressed", String(picked));
    return b;
  };
  const tabs = [
    tab("Off", on === null, () => { void save(null, ""); }),
    ...view.offered.map(o => tab(o.name, on === o.provider, () => { if (on !== o.provider) void save(o.provider, ""); })),
  ];
  const hint = on === null ? "Off: the key does nothing" : `Uses your ${active(view)!.name} key`;
  return h("div", "cc-row cc-line bare", h("span", "t", h("b", "", "Provider"), h("span", "", hint)), h("span", "r", ...tabs));
}

function typedModelRow(view: DictationView) {
  const on = active(view)!;
  const input = h("input", "dsecret");
  input.value = shownModel(view);
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", "Model");
  const commit = () => {
    const typed = input.value.trim();
    if (typed !== shownModel(view)) void save(on.provider, typed === on.default_model ? "" : typed);
  };
  input.addEventListener("change", commit);
  input.addEventListener("keydown", e => { if (e.key === "Enter") commit(); });
  return h("div", "cc-row cc-line bare", h("span", "t", h("b", "", "Model"), h("span", "", `Default: ${on.default_model}`)), h("span", "r", input));
}

function modelList(view: DictationView) {
  const search = h("input", "dsecret");
  search.type = "search";
  search.placeholder = "Search models";
  search.value = state.query;
  search.autocomplete = "off";
  search.spellcheck = false;
  search.setAttribute("aria-label", "Search models");
  const list = h("div", "");
  const fill = () => {
    if (state.models === undefined) return list.replaceChildren(h("div", "cc-row cc-empty", h("span", "", "Loading models…")));
    if (state.models === null) return list.replaceChildren(h("div", "cc-row cc-empty", h("span", "", "Could not list the models. Type one below.")));
    const { rows, more } = filterModels(state.models, state.query);
    const picked = shownModel(view);
    const rowEls = rows.map(m => {
      const row = h("button", "cc-row cc-line bare", h("span", "t", h("b", "", m.name), h("span", "", m.id)), h("span", "r", m.id === picked ? "Selected" : ""));
      row.type = "button";
      if (m.id === picked) row.setAttribute("aria-current", "true");
      row.addEventListener("click", () => { void save("openrouter", m.id); });
      return row;
    });
    const rest = more > 0 ? [h("div", "cc-row cc-empty", h("span", "", `${more} more, search to narrow`))] : [];
    list.replaceChildren(...rowEls, ...rest);
    if (!rows.length) list.replaceChildren(h("div", "cc-row cc-empty", h("span", "", "No model matches")));
  };
  search.addEventListener("input", () => { state.query = search.value; fill(); });
  fill();
  return h("div", "", h("div", "", search), list);
}

function draw() {
  const view = state.view;
  const head = secHead("Dictation", h("small", "", DICTATION_NOTE));
  let body: HTMLElement;
  if (view === undefined) body = h("div", "cc-panel cc-empty", h("span", "", "Loading…"));
  else if (view === null) body = h("div", "cc-panel cc-empty", h("span", "", "Could not read your dictation settings."));
  else if (!view.offered.length) body = h("div", "cc-panel cc-empty", h("span", "", NO_KEYS));
  else {
    const on = active(view);
    body = h("div", "cc-panel", providerRow(view), on?.provider === "openrouter" ? modelList(view) : on ? typedModelRow(view) : null);
  }
  section.replaceChildren(head, body, ...(state.error ? [h("div", "derr", state.error)] : []));
}

/// The section. Asking for it loads the saved pick and the providers with a key again, so a key
/// saved or removed above shows at once.
export function dictation() {
  draw();
  void load();
  return section;
}
