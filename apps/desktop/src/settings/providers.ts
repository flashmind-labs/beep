import { createElement } from "react";
import { removeWindowPart, showWindowParts } from "../center/window-root.ts";
import { ProviderHeader, ProvidersView } from "./providers-view.tsx";
// The Coding agents page: per machine, each harness (Claude Code, Codex, pi,
// ...) with its version, sign-in and an on/off switch, and the picked one's
// runtime settings.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { MakeButton, MakeEl } from "../ui/dom.ts";
import { checkedLabel, compactKind, compactOf, compactProblem, differsLabel, draftOf, harnessName, hasCompact, listRows, savedToLabel, settingsInput, toggledInput, updatable } from "./providers-state.ts";
import type { Compact, HarnessDraft, HarnessesView, HarnessView, MachineSettings } from "./providers-state.ts";
import type { CenterView } from "../center/state.ts";
import { modelContextPanel } from "./model-context.ts";
import { flshPanel } from "./flsh.ts";
import { type AdapterInstall, adapterOf } from "../machines/adapters-state.ts";

/// A machine as `list_machines` gives it; only these fields are read.
interface MachineRow { id: string; name: string; online: boolean; can_host?: boolean; adapters?: AdapterInstall[] }

/// What the window hands `registerProviders`.
interface ProvidersDeps {
  registerView: (name: "providers", view: CenterView<"providers">) => void;
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  /// Opens Settings at a section, such as "agents".
  openSettings: (section?: string) => void;
  /// Redraws the window's header, where the machine chips are.
  redrawTop: () => void;
}

/// The page's state.
interface ProvidersState {
  machines: MachineRow[] | null | undefined;
  machine: string | null;
  views: Map<string, HarnessesView | null | false>;
  errors: Map<string, string>;
  picked: string | null;
  drafts: Map<string, HarnessDraft>;
  updating: Set<string>;
  saving: boolean;
  note: string;
  /// Each machine's saved settings, by agent, to say which fields differ
  /// (false from a server too old to list them).
  others: Map<string, MachineSettings[] | false>;
  /// Each agent's saved auto-compact, the same on every machine.
  compacts: Map<string, Compact>;
  /// "Save to all hosts" copies the binary path too.
  includeProgram: boolean;
  /// What the last save to all hosts did, by machine and agent.
  savedAll: Map<string, string>;
}

export function registerProviders(deps: ProvidersDeps) {
  const { registerView, invoke, h, button } = deps;
  const st: ProvidersState = { machines: undefined, machine: null, views: new Map(), errors: new Map(), picked: null, drafts: new Map(), updating: new Set(), saving: false, note: "", others: new Map(), compacts: new Map(), includeProgram: false, savedAll: new Map() };
  /// The agents whose machines' settings are being read.
  const reading = new Set<string>();
  const readingCompact = new Set<string>();
  const loading = new Map<string, number>();
  const readVersion = new Map<string, number>();
  const isLoading = () => !!st.machine && (loading.get(st.machine) ?? 0) > 0;
  const compactDrafts = new Map<string, Pick<HarnessDraft, "compactOn" | "compactPercent">>();
  const compactErrors = new Map<string, string>();
  const savingCompacts = new Set<string>();
  let root: HTMLElement | null = null;
  let cachedShared: { key: string; node: { before: HTMLElement; after: HTMLElement; agent: string } } | null = null;
  /// Drawn in Settings, which lists the agents in its own left pane: the
  /// page shows the one agent it was opened at, under a toolbar of its own.
  let inSettings = false;
  const contexts = modelContextPanel(invoke);
  const flsh = flshPanel(invoke);

  const fail = (e: unknown) => String(e).replace(/^(bad request|not found|error):\s*/i, "");
  const view = () => (st.machine ? st.views.get(st.machine) : undefined);
  const key = (agent: string) => `${st.machine} ${agent}`;
  const machineName = () => st.machines?.find(m => m.id === st.machine)?.name ?? "this machine";

  async function loadMachines() {
    try {
      const all = await invoke<MachineRow[]>("list_machines");
      st.machines = all.filter(m => m.can_host !== false);
      const online = st.machines.filter(m => m.online);
      if (!st.machine || !online.some(m => m.id === st.machine)) st.machine = online[0]?.id ?? null;
    } catch (e) {
      st.machines = null;
      st.note = fail(e);
    }

    deps.redrawTop();
    draw();
    if (st.machine) void load(false);
  }

  async function load(refresh: boolean, machine = st.machine) {
    if (!machine) return;
    const version = (readVersion.get(machine) ?? 0) + 1;
    readVersion.set(machine, version);
    loading.set(machine, (loading.get(machine) ?? 0) + 1);
    draw();

    try {
      const got = await invoke<HarnessesView | null>("agent_harnesses", { machineId: machine, refresh });
      if (readVersion.get(machine) === version) { st.views.set(machine, got ?? false); st.errors.delete(machine); }
    } catch (e) {
      if (readVersion.get(machine) === version) {
        if (!st.views.get(machine)) st.views.set(machine, null);
        st.errors.set(machine, fail(e));
      }
    } finally {
      const remaining = (loading.get(machine) ?? 1) - 1;
      if (remaining) loading.set(machine, remaining); else loading.delete(machine);
    }

    deps.redrawTop();
    draw();
  }

  function pickMachine(id: string) {
    if (st.machine === id) return;
    st.machine = id;
    // In Settings the left pane picked the agent; it stays across machines.
    if (!inSettings) st.picked = null;
    deps.redrawTop();
    draw();
    if (!st.views.has(id)) load(false);
  }

  /// How `agent`'s adapter stands on the machine picked, when it is pending.
  const adapter = (agent: string) => adapterOf(st.machines?.find(m => m.id === st.machine)?.adapters, agent);

  /// Try again: the machine installs the adapters it could not, now.
  async function retryAdapters(agent: string) {
    const machine = st.machine;
    if (!machine) return;
    st.updating.add(`${machine} ${agent}`);
    draw();

    try {
      await invoke("agent_adapters_install", { machineId: machine });
      // The node reads them as installing at once; its report follows.
      for (const a of st.machines?.find(m => m.id === machine)?.adapters ?? []) a.failed = null;
      st.errors.delete(machine);
    } catch (e) {
      st.errors.set(machine, fail(e));
    } finally {
      st.updating.delete(`${machine} ${agent}`);
    }

    draw();
  }

  async function update(agents: string[]) {
    const machine = st.machine;
    if (!machine) return;
    for (const agent of agents) st.updating.add(`${machine} ${agent}`);
    deps.redrawTop();
    draw();

    for (const agent of agents) {
      try {
        st.views.set(machine, await invoke<HarnessesView>("agent_harness_update", { machineId: machine, agent }));
        st.errors.delete(machine);
      } catch (e) {
        st.errors.set(machine, fail(e));
      } finally {
        st.updating.delete(`${machine} ${agent}`);
        deps.redrawTop();
        draw();
      }
    }
  }

  /// Saves the shared auto-compact setting.
  async function saveCompact(agent: string, compact: Compact) {
    await invoke("harness_compact_save", { agent, compact });
    st.compacts.delete(agent);
  }

  async function save(hv: HarnessView, input: ReturnType<typeof settingsInput>) {
    const machine = st.machine;
    if (!machine || st.saving) return;
    const submitted = st.drafts.get(`${machine} ${hv.agent}`);
    st.saving = true;
    draw();

    try {
      await invoke("harness_settings_save", { machineId: machine, agent: hv.agent, settings: input });
      if (st.drafts.get(`${machine} ${hv.agent}`) === submitted) st.drafts.delete(`${machine} ${hv.agent}`);
      st.savedAll.delete(`${machine} ${hv.agent}`);
      st.others.delete(hv.agent);
      st.errors.delete(machine);
      // Read back what the server saved, without asking npm again.
      await load(false, machine);
    } catch (e) {
      st.errors.set(machine, fail(e));
    } finally {
      st.saving = false;
    }

    draw();
  }

  /// Reads `agent`'s settings on every machine, once, for the differs notes.
  async function loadOthers(agent: string) {
    if (reading.has(agent) || st.others.has(agent)) return;
    reading.add(agent);

    try {
      st.others.set(agent, (await invoke<MachineSettings[] | null>("harness_settings_all", { agent })) ?? false);
    } catch (e) {
      // The notes are a hint; the page works without them.
      console.warn("harness settings on every machine:", e);
      st.others.set(agent, false);
    } finally {
      reading.delete(agent);
    }

    draw();
  }

  /// Reads `agent`'s saved auto-compact once.
  async function loadCompact(agent: string) {
    if (readingCompact.has(agent) || st.compacts.has(agent)) return;
    readingCompact.add(agent);

    try {
      st.compacts.set(agent, await invoke<Compact>("harness_compact", { agent }));
    } catch (e) {
      console.warn("auto-compact setting:", e);
      st.compacts.set(agent, null);
    } finally {
      readingCompact.delete(agent);
    }

    draw();
  }

  /// Saves the form for `hv` on every machine of the user's, online or not.
  async function saveAll(hv: HarnessView, input: ReturnType<typeof settingsInput>) {
    const machine = st.machine;
    if (!machine || st.saving) return;
    const submitted = new Map(st.drafts);
    st.saving = true;
    draw();

    try {
      const n = await invoke<number>("harness_settings_save_all", { fromMachine: machine, agent: hv.agent, settings: input, includeProgram: st.includeProgram });
      // Drafts on the other machines were of the settings before.
      for (const k of [...st.drafts.keys()]) if (k.endsWith(` ${hv.agent}`) && st.drafts.get(k) === submitted.get(k)) st.drafts.delete(k);
      st.savedAll.set(`${machine} ${hv.agent}`, savedToLabel(n));
      st.errors.delete(machine);
      st.others.delete(hv.agent);
      // The other machines' lists are read again when shown.
      for (const id of [...st.views.keys()]) if (id !== machine) st.views.delete(id);
      const current = st.machine;
      await Promise.all([load(false, machine), ...(current && current !== machine ? [load(false, current)] : [])]);
    } catch (e) {
      st.errors.set(machine, fail(e));
    } finally {
      st.saving = false;
    }

    draw();
  }

  /// A setting: its label and help on the left, `control` on the right.
  function field(label: string, help: string, control: HTMLElement, names: readonly string[] = []) {
    const note = names.length ? h("span", "hv-diff", differsLabel(names)) : null;
    note?.setAttribute("title", `Not the same on ${names.join(", ")}`);
    return h("div", "hv-f", h("span", "", h("b", "", label), note, h("small", "", help)), control);
  }

  function input(value: string, placeholder: string, onInput: (v: string) => void, label: string) {
    const i = h("input", "hv-in");
    i.value = value;
    i.placeholder = placeholder;
    i.spellcheck = false;
    i.autocomplete = "off";
    i.setAttribute("aria-label", label);
    i.addEventListener("input", () => onInput(i.value));
    return i;
  }

  function compactSwitch(on: boolean, label: string, flip: () => void) {
    const b = h("button", "dswitch");
    b.type = "button";
    b.setAttribute("role", "switch");
    b.setAttribute("aria-checked", String(on));
    b.setAttribute("aria-label", label);
    b.addEventListener("click", flip);
    return b;
  }

  function sharedSettings(agent: string) {
    const box = h("div", "", h("div", "hv-note", "These settings apply on every machine."));
    box.style.display = "contents";

    if (hasCompact(agent)) {
      const loaded = st.compacts.has(agent);
      if (!loaded) void loadCompact(agent);
      const saved = st.compacts.get(agent) ?? null;
      const initial = { compactOn: saved?.enabled ?? true, compactPercent: saved?.tokens != null ? String(saved.tokens) : saved?.percent != null ? String(saved.percent) : "" };
      const draft = compactDrafts.get(agent) ?? initial;
      const busy = savingCompacts.has(agent);
      const problem = h("span", "derr", compactErrors.get(agent) ?? "");
      const saveBtn = button(busy ? "Saving…" : "Save auto-compact", "pri", async () => {
        if (!loaded || busy || compactProblem(draft)) return;
        savingCompacts.add(agent);
        compactErrors.delete(agent);
        draw();

        try {
          await saveCompact(agent, compactOf(draft));
          compactDrafts.delete(agent);
          await loadCompact(agent);
        } catch (e) {
          compactErrors.set(agent, fail(e));
        } finally {
          savingCompacts.delete(agent);
          draw();
        }
      });
      const refresh = () => {
        problem.textContent = compactProblem(draft) || (compactErrors.get(agent) ?? "");
        saveBtn.disabled = !loaded || busy || !!compactProblem(draft) || JSON.stringify(compactOf(draft)) === JSON.stringify(saved);
      };
      const change = (value: Partial<typeof draft>) => {
        Object.assign(draft, value);
        compactDrafts.set(agent, draft);
        discard.hidden = false;
        refresh();
      };
      const toggle = compactSwitch(draft.compactOn, `Auto-compact for ${harnessName(agent)}`, () => { change({ compactOn: !draft.compactOn }); draw(); });
      toggle.disabled = !loaded || busy;
      const threshold = input(draft.compactPercent, "80% or 350000", value => change({ compactPercent: value }), "Auto-compact threshold");
      threshold.disabled = !loaded || busy;
      const discard = button("Discard", "quiet", () => { compactDrafts.delete(agent); compactErrors.delete(agent); draw(); });
      discard.disabled = busy;
      discard.hidden = !compactDrafts.has(agent);
      box.append(field("Auto-compact", "Applies to sessions on every machine.", h("span", "fl-row", toggle, compactKind(agent) === "percent" ? threshold : null)),
        h("div", "hv-acts", saveBtn, discard, problem));
      refresh();
    }

    const after = h("div");
    after.style.display = "contents";
    if (st.machine) {
      after.append(h("div", "hv-note", `Model choices and reported limits come from ${machineName()}.`));
    }
    return { before: box, after, agent };
  }

  function draw(preserveShared = false) {
    if (!root) return;
    const current = view();
    if (!inSettings && current && st.machine) {
      const rows = listRows(current);
      if (!st.picked || !rows.some(r => r.agent === st.picked)) st.picked = rows.find(r => r.on)?.agent ?? rows[0]?.agent ?? null;
    }
    const hv = current ? current.harnesses.find(item => item.agent === st.picked) : undefined;
    if (hv?.installed && st.others.get(hv.agent) === undefined) void loadOthers(hv.agent);
    const sharedAgent = inSettings ? st.picked : hv?.agent ?? null;
    const sharedKey = sharedAgent ? `${st.machine ?? ""}\0${sharedAgent}\0${inSettings}` : "";
    if (!sharedAgent) cachedShared = null;
    else if (!preserveShared || cachedShared?.key !== sharedKey) cachedShared = { key: sharedKey, node: sharedSettings(sharedAgent) };
    const machine = st.machine;
    const draftKey = hv ? key(hv.agent) : "";
    const draft = hv?.installed ? st.drafts.get(draftKey) ?? draftOf(hv) : null;
    const behind = updatable(current || null);
    const updateCount = [...st.updating].filter(k => k.startsWith(`${machine} `)).length;
    showWindowParts([{ part: "settings-page:providers", host: root, content: createElement(ProvidersView, {
      machines: st.machines, machine, machineName: machineName(), view: current, picked: st.picked,
      draft, others: hv ? st.others.get(hv.agent) : undefined, saving: st.saving,
      updating: hv ? st.updating.has(key(hv.agent)) : false, adapter: hv ? adapter(hv.agent) : null,
      error: st.machines === null ? st.note : machine ? st.errors.get(machine) ?? "" : st.note, inSettings, savedAll: st.savedAll.get(draftKey) ?? "",
      includeProgram: st.includeProgram, shared: cachedShared?.node ?? null, flsh, contexts,
      loading: isLoading(), checked: checkedLabel(current ? current.checked_at : null, Date.now()), updateCount, canUpdate: !!behind.length,
      pick: (agent: string) => { st.picked = agent; draw(); },
      toggle: (item: HarnessView, on: boolean) => { void save(item, toggledInput(item, on)); },
      update: (item: HarnessView) => { void update([item.agent]); }, retryAdapter: (agent: string) => { void retryAdapters(agent); },
      openAccounts: () => deps.openSettings("agents"),
      setDraft: (value: Partial<HarnessDraft>) => { if (!hv || !draft) return; st.drafts.set(draftKey, { ...draft, ...value }); draw(true); },
      save: (item: HarnessView) => { if (draft) void save(item, settingsInput(item.settings.enabled, draft)); },
      saveAll: (item: HarnessView) => { if (draft) void saveAll(item, settingsInput(item.settings.enabled, draft)); },
      discard: () => { st.drafts.delete(draftKey); draw(true); },
      setIncludeProgram: (value: boolean) => { st.includeProgram = value; draw(true); },
      dismissError: () => { if (machine) st.errors.delete(machine); draw(true); },
      pickMachine, refresh: () => { void load(true); }, updateAll: () => { void update(behind); },
    }) }]);
  }

  registerView("providers", {
    // In Settings (`arg.settings`) its buttons sit in the page's own toolbar.
    title: () => {
      const current = view();
      return {
        title: "Coding agents", sub: "What each machine can run",
        reactTools: inSettings ? null : createElement(ProviderHeader, {
          machines: st.machines, machine: st.machine, loading: isLoading(),
          checked: checkedLabel(current ? current.checked_at : null, Date.now()),
          updateCount: [...st.updating].filter(key => key.startsWith(`${st.machine} `)).length,
          canUpdate: !!updatable(current || null).length,
          pickMachine, refresh: () => { void load(true); },
          updateAll: () => { void update(updatable(view() || null)); },
        }),
      };
    },
    // Settings opens it at one agent (`arg.agent`), which its left pane picks.
    mount(host, arg) {
      removeWindowPart("settings-page:providers");
      host.replaceChildren();
      root = host;
      cachedShared = null;
      const want = arg;
      if (want?.agent) st.picked = want.agent;
      inSettings = want?.settings === true;
      draw();
      loadMachines();
    },
    unmount() { removeWindowPart("settings-page:providers"); root = null; cachedShared = null; },
  });
}
