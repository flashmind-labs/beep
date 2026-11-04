// flsh's own settings on its Coding agents page: OpenRouter routing and
// auto-compact, the same on every machine.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { MakeButton, MakeEl } from "../ui/dom.ts";
import { EMPTY, QUANTIZATIONS, SORTS, flshChanged, flshDraft, flshInput, flshProblem, toggleQuantization } from "./flsh-state.ts";
import type { FlshDraft, FlshSettings, Sort } from "./flsh-state.ts";

interface FlshDeps {
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  /// Draws the page again.
  redraw: () => void;
}

const DEFAULT_TRIGGER = "Model's default trigger";

export function flshPanel(deps: FlshDeps) {
  const { invoke, h, button, redraw } = deps;
  /// undefined while loading, false from a server too old to keep them.
  let saved: FlshSettings | false | undefined;
  let draft: FlshDraft | null = null;
  let error = "";
  let saving = false;
  let asked = false;

  const fail = (e: unknown) => String(e).replace(/^(bad request|not found|error):\s*/i, "");

  async function load() {
    asked = true;
    try {
      saved = (await invoke<FlshSettings | null>("flsh_settings_get")) ?? false;
      error = "";
    } catch (e) {
      saved = EMPTY;
      error = fail(e);
    }
    draft = null;
    redraw();
  }

  async function save() {
    if (!draft || saving) return;
    saving = true;
    redraw();
    try {
      await invoke("flsh_settings_save", { settings: flshInput(draft) });
      await load();
    } catch (e) {
      error = fail(e);
    } finally {
      saving = false;
    }
    redraw();
  }

  function field(label: string, help: string, ctl: Node) {
    return h("div", "hv-f", h("span", "", h("b", "", label), h("small", "", help)), ctl);
  }

  /// A row of chips, pressed where `pressed` says; `on` takes a click.
  function chips<T>(cls: string, label: string, opts: [string, T][], pressed: (v: T) => boolean, on: (v: T) => void) {
    const row = h("span", cls);
    row.setAttribute("role", "group");
    row.setAttribute("aria-label", label);
    for (const [text, v] of opts) {
      const b = button(text, "dchip", () => on(v));
      b.className = "dchip";
      b.setAttribute("aria-pressed", String(pressed(v)));
      row.append(b);
    }
    return row;
  }

  /// A row of chips where one is pressed; `on` picks it.
  const seg = <T>(opts: [string, T][], now: T, label: string, on: (v: T) => void) => chips("fl-seg", label, opts, v => v === now, on);

  function input(value: string, placeholder: string, label: string, onInput: (v: string) => void) {
    const i = h("input", "hv-in");
    i.value = value;
    i.placeholder = placeholder;
    i.spellcheck = false;
    i.autocomplete = "off";
    i.setAttribute("aria-label", label);
    i.addEventListener("input", () => onInput(i.value));
    return i;
  }

  function sw(on: boolean, label: string, flip: () => void) {
    const b = h("button", "dswitch");
    b.type = "button";
    b.setAttribute("role", "switch");
    b.setAttribute("aria-checked", String(on));
    b.setAttribute("aria-label", label);
    b.addEventListener("click", flip);
    return b;
  }

  /// The section's nodes, for flsh's page.
  function render(): Node[] {
    if (!asked) load();
    if (saved === undefined) return [h("h4", "", "OpenRouter routing"), h("div", "hv-note", "Loading flsh's settings…")];
    if (saved === false) return [h("h4", "", "OpenRouter routing"), h("div", "hv-note", "The server needs an update before it keeps flsh's settings.")];
    const base = saved;
    const d: FlshDraft = draft ?? flshDraft(base);
    const set = (change: Partial<FlshDraft>, again = true) => {
      draft = { ...d, ...change };
      Object.assign(d, change);
      if (again) redraw(); else refresh();
    };

    const out: Node[] = [h("h4", "", "OpenRouter routing"),
      h("div", "hv-note", "Applies to every machine.")];

    out.push(field("Zero data retention", "Only endpoints that keep no prompts.",
      seg<boolean | null>([["Account setting", null], ["Require", true], ["Off", false]], d.zdr, "Zero data retention", v => set({ zdr: v }))));

    const quants = chips("fl-chips", "Allowed quantizations", QUANTIZATIONS.map(q => [q, q]), q => d.quantizations.includes(q),
      q => set({ quantizations: toggleQuantization(d, q) }));
    out.push(field("Allowed quantizations", d.quantizations.length ? "Weight precisions providers may serve." : "None picked: any precision.", quants));

    const sorts: [string, Sort | null][] = [["Default", null], ...SORTS.map(s => [s[0].toUpperCase() + s.slice(1), s] as [string, Sort])];
    out.push(field("Sort providers by", "Default: OpenRouter balances price and uptime.",
      seg(sorts, d.sort, "Sort providers by", v => set({ sort: v }))));
    out.push(field("Allow fallbacks", "Try other providers when the first ones fail.",
      seg<boolean | null>([["On (OpenRouter default)", null], ["Off", false]], d.allow_fallbacks === true ? null : d.allow_fallbacks, "Allow fallbacks", v => set({ allow_fallbacks: v }))));

    out.push(h("h4", "", "Auto-compact"));
    const global = h("span", "fl-row",
      sw(d.compact_on, "Auto-compact for all models", () => set({ compact_on: !d.compact_on })),
      input(d.compact_tokens, d.compact_on ? DEFAULT_TRIGGER : "Off: waits for the window to overflow", "Auto-compact threshold", v => set({ compact_tokens: v }, false)));
    out.push(field("All models", "Compact once the prompt passes this many tokens.", global));

    const table = h("table", "fl-tbl");
    table.append(h("tr", "", h("th", "", "Model"), h("th", "", "On"), h("th", "", "Threshold (tokens)"), h("th", "")));
    d.models.forEach((m, i) => {
      const models = () => d.models.map(x => ({ ...x }));
      const remove = button("×", "quiet small", () => set({ models: d.models.filter((_, j) => j !== i) }));
      remove.setAttribute("aria-label", `Remove ${m.model}`);
      table.append(h("tr", "",
        h("td", "", input(m.model, "provider:model", `Model ${i + 1}`, v => { const all = models(); all[i].model = v; set({ models: all }, false); })),
        h("td", "", sw(m.enabled, `Auto-compact for model ${i + 1}`, () => { const all = models(); all[i].enabled = !m.enabled; set({ models: all }); })),
        h("td", "", input(m.tokens, m.enabled ? DEFAULT_TRIGGER : "Off", `Threshold for model ${i + 1}`, v => { const all = models(); all[i].tokens = v; set({ models: all }, false); })),
        h("td", "", remove)));
    });
    const add = button("Add model", "small", () => set({ models: [...d.models, { model: "", enabled: true, tokens: "" }] }));
    table.append(h("tr", "", h("td", "", h("span", "hv-note", d.models.length ? "" : "No model has its own rule.")), h("td", ""), h("td", ""), h("td", "", add)));
    out.push(table);

    const problem = h("span", "derr", "");
    const saveBtn = button(saving ? "Saving…" : "Save", "pri", () => { if (!flshProblem(d)) save(); });
    const discard = button("Discard", "quiet", () => { draft = null; error = ""; redraw(); });
    function refresh() {
      const changed = flshChanged(base, d);
      const why = flshProblem(d);
      problem.textContent = why || error;
      saveBtn.disabled = saving || !changed || !!why;
      discard.hidden = !changed;
    }
    refresh();
    out.push(h("div", "hv-acts", saveBtn, discard, problem));
    return out;
  }

  return { render };
}
