import { removeWindowPart, showWindowParts } from "../center/window-root.tsx";
import { ProviderHeader, ProvidersView, type SharedProviderSettings } from "./providers-view.tsx";
// The Coding agents page: per machine, each harness (Claude Code, Codex, pi,
// ...) with its version, sign-in and an on/off switch, and the picked one's
// runtime settings.
import type { invoke as Invoke } from "../ui/tauri.ts";
import { pushedEach } from "../ui/snapshot-state.ts";
import { checkedLabel, compactDraftOf, compactOf, compactProblem, draftOf, hasCompact, listRows, savedToLabel, settingsInput, toggledInput, updatable } from "./providers-state.ts";
import type { AutoCompactDraft, Compact, HarnessDraft, HarnessesView, HarnessView, MachineSettings } from "./providers-state.ts";
import type { CenterView } from "../center/state.ts";
import { modelContextPanel } from "./model-context.ts";
import { flshPanel } from "./flsh.ts";
import { type AdapterInstall, adapterOf } from "../machines/adapters-state.ts";
import { ensureMachines, MACHINES } from "../machines/machine-store.ts";

/// A machine as `list_machines` gives it; only these fields are read.
interface MachineRow { id: string; name: string; online: boolean; can_host?: boolean; adapters?: AdapterInstall[] }

/// What the window hands `providersPage`.
interface ProvidersDeps {
  invoke: typeof Invoke;
  /// Opens Settings at a section, such as "agents".
  openSettings: (section?: string) => void;
  /// Redraws the window's header, where the machine chips are.
  redrawTop: () => void;
  /// Calls `take` with every `machines` and `accounts` update, which carry each machine's harnesses.
  follow: (take: (data: unknown) => void) => void;
}

/// The page's state.
interface ProvidersState {
  machine: string | null;
  views: Map<string, HarnessesView | null>;
  errors: Map<string, string>;
  picked: string | null;
  drafts: Map<string, HarnessDraft>;
  updating: Set<string>;
  saving: boolean;
  note: string;
  /// Each machine's saved settings, by agent, to say which fields differ
  /// (false when they could not be read).
  others: Map<string, MachineSettings[] | false>;
  /// Each agent's saved auto-compact, the same on every machine.
  compacts: Map<string, Compact>;
  /// "Save to all hosts" copies the binary path too.
  includeProgram: boolean;
  /// What the last save to all hosts did, by machine and agent.
  savedAll: Map<string, string>;
}

export function providersPage(deps: ProvidersDeps): CenterView<"providers"> {
  const { invoke } = deps;
  const st: ProvidersState = { machine: null, views: new Map(), errors: new Map(), picked: null, drafts: new Map(), updating: new Set(), saving: false, note: "", others: new Map(), compacts: new Map(), includeProgram: false, savedAll: new Map() };
  /// The agents whose machines' settings are being read.
  const reading = new Set<string>();
  const readingCompact = new Set<string>();
  const loading = new Map<string, number>();
  const readVersion = new Map<string, number>();
  /// The machines that run agents, from the held list: undefined before it is read, null when it could not be.
  const machines = (): MachineRow[] | null | undefined => MACHINES.list ? MACHINES.list.filter(m => m.can_host !== false) : MACHINES.failed ? null : undefined;
  const isLoading = () => !!st.machine && (loading.get(st.machine) ?? 0) > 0;
  const compactDrafts = new Map<string, AutoCompactDraft>();
  const compactErrors = new Map<string, string>();
  const savingCompacts = new Set<string>();
  let root: HTMLElement | null = null;
  /// Drawn in Settings, which lists the agents in its own left pane: the
  /// page shows the one agent it was opened at, under a toolbar of its own.
  let inSettings = false;
  const contexts = modelContextPanel(invoke);
  const flsh = flshPanel(invoke);
  /// A machine's agents stand until the push brings them again.
  deps.follow(data => {
    for (const { args, pushed } of pushedEach<HarnessesView>(data, "agent_harnesses")) {
      const machine = args.machineId;
      if (typeof machine !== "string" || (args.refresh === true)) continue;
      // A read of this machine that is out answers older than the push.
      readVersion.set(machine, (readVersion.get(machine) ?? 0) + 1);

      if ("error" in pushed) { st.errors.set(machine, fail(pushed.error)); if (!st.views.get(machine)) st.views.set(machine, null); }
      else { st.views.set(machine, pushed.value); st.errors.delete(machine); }
    }

    if (root) { deps.redrawTop(); draw(); }
  });
  /// Reads machine `id`'s agents the first time; after that the push keeps them.
  const ensure = (id: string | null) => { if (id && (!st.views.has(id) || st.views.get(id) === null)) void load(false, id); };

  const fail = (e: unknown) => String(e).replace(/^(bad request|not found|error):\s*/i, "");
  const view = () => (st.machine ? st.views.get(st.machine) : undefined);
  const key = (agent: string) => `${st.machine} ${agent}`;
  const machineName = () => machines()?.find(m => m.id === st.machine)?.name ?? "this machine";

  async function loadMachines() {
    try {
      await ensureMachines();
      const online = (machines() ?? []).filter(m => m.online);
      if (!st.machine || !online.some(m => m.id === st.machine)) st.machine = online[0]?.id ?? null;
    } catch (e) {
      st.note = fail(e);
    }

    deps.redrawTop();
    draw();
    ensure(st.machine);
  }

  async function load(refresh: boolean, machine = st.machine) {
    if (!machine) return;
    const version = (readVersion.get(machine) ?? 0) + 1;
    readVersion.set(machine, version);
    loading.set(machine, (loading.get(machine) ?? 0) + 1);
    draw();

    try {
      const got = await invoke<HarnessesView>("agent_harnesses", { machineId: machine, refresh });
      if (readVersion.get(machine) === version) { st.views.set(machine, got); st.errors.delete(machine); }
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
    ensure(id);
  }

  /// How `agent`'s adapter stands on the machine picked, when it is pending.
  const adapter = (agent: string) => adapterOf(machines()?.find(m => m.id === st.machine)?.adapters, agent);

  /// Try again: the machine installs the adapters it could not, now.
  async function retryAdapters(agent: string) {
    const machine = st.machine;
    if (!machine) return;
    st.updating.add(`${machine} ${agent}`);
    draw();

    try {
      await invoke("agent_adapters_install", { machineId: machine });
      // The node reads them as installing at once; its report follows.
      for (const a of machines()?.find(m => m.id === machine)?.adapters ?? []) a.failed = null;
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
      // Read back what was saved, without asking npm again.
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
      st.others.set(agent, await invoke<MachineSettings[]>("harness_settings_all", { agent }));
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

  async function saveSharedCompact(agent: string) {
    const current = sharedSettings(agent).compact;
    if (!current || !current.loaded || current.busy || compactProblem(current.draft)) return;
    const submitted = current.draft;
    savingCompacts.add(agent);
    compactErrors.delete(agent);
    draw();
    try {
      await invoke("harness_compact_save", { agent, compact: compactOf(submitted) });
      st.compacts.delete(agent);
      if (compactDrafts.get(agent) === submitted) compactDrafts.delete(agent);
      await loadCompact(agent);
    } catch (error) {
      compactErrors.set(agent, fail(error));
    } finally {
      savingCompacts.delete(agent);
      draw();
    }
  }

  function changeCompact(agent: string, change: Partial<AutoCompactDraft>) {
    const current = sharedSettings(agent).compact;
    if (!current || current.busy) return;
    compactDrafts.set(agent, { ...current.draft, ...change });
    draw();
  }

  function discardCompact(agent: string) {
    compactDrafts.delete(agent);
    compactErrors.delete(agent);
    draw();
  }

  function sharedSettings(agent: string): SharedProviderSettings {
    const saved = st.compacts.get(agent) ?? null;
    const draft = compactDrafts.get(agent) ?? compactDraftOf(saved);
    return { agent, machineName: st.machine ? machineName() : null, compact: hasCompact(agent) ? {
      loaded: st.compacts.has(agent), draft, busy: savingCompacts.has(agent), edited: compactDrafts.has(agent),
      error: compactProblem(draft) || (compactErrors.get(agent) ?? ""),
      canSave: st.compacts.has(agent) && !savingCompacts.has(agent) && !compactProblem(draft) && JSON.stringify(compactOf(draft)) !== JSON.stringify(saved),
    } : null };
  }

  function draw() {
    if (!root) return;
    const current = view();
    if (!inSettings && current && st.machine) {
      const rows = listRows(current);
      if (!st.picked || !rows.some(r => r.agent === st.picked)) st.picked = rows.find(r => r.on)?.agent ?? rows[0]?.agent ?? null;
    }
    const hv = current ? current.harnesses.find(item => item.agent === st.picked) : undefined;
    const sharedAgent = inSettings ? st.picked : hv?.agent ?? null;
    const machine = st.machine;
    const draftKey = hv ? key(hv.agent) : "";
    const draft = hv?.installed ? st.drafts.get(draftKey) ?? draftOf(hv) : null;
    const behind = updatable(current || null);
    const updateCount = [...st.updating].filter(k => k.startsWith(`${machine} `)).length;
    showWindowParts([{ part: "settings-page:providers", host: root, content: <ProvidersView
      machines={machines()} machine={machine} machineName={machineName()} view={current} picked={st.picked}
      draft={draft} others={hv ? st.others.get(hv.agent) : undefined} saving={st.saving}
      updating={hv ? st.updating.has(key(hv.agent)) : false} adapter={hv ? adapter(hv.agent) : null}
      error={machines() === null ? st.note : machine ? st.errors.get(machine) ?? "" : st.note} inSettings={inSettings} savedAll={st.savedAll.get(draftKey) ?? ""}
      includeProgram={st.includeProgram} shared={sharedAgent ? sharedSettings(sharedAgent) : null} flsh={flsh} contexts={contexts} invoke={invoke}
      loadOthers={loadOthers} loadCompact={loadCompact} changeCompact={changeCompact} saveCompact={saveSharedCompact} discardCompact={discardCompact}
      loading={isLoading()} checked={checkedLabel(current ? current.checked_at : null, Date.now())} updateCount={updateCount} canUpdate={!!behind.length}
      pick={(agent: string) => { st.picked = agent; draw(); }}
      toggle={(item: HarnessView, on: boolean) => { void save(item, toggledInput(item, on)); }}
      update={(item: HarnessView) => { void update([item.agent]); }} retryAdapter={(agent: string) => { void retryAdapters(agent); }}
      openAccounts={() => deps.openSettings("agents")}
      setDraft={(value: Partial<HarnessDraft>) => { if (!hv || !draft) return; st.drafts.set(draftKey, { ...draft, ...value }); draw(); }}
      save={(item: HarnessView) => { if (draft) void save(item, settingsInput(item.settings.enabled, draft)); }}
      saveAll={(item: HarnessView) => { if (draft) void saveAll(item, settingsInput(item.settings.enabled, draft)); }}
      discard={() => { st.drafts.delete(draftKey); draw(); }}
      setIncludeProgram={(value: boolean) => { st.includeProgram = value; draw(); }}
      dismissError={() => { if (machine) st.errors.delete(machine); draw(); }}
      pickMachine={pickMachine} refresh={() => { void load(true); }} updateAll={() => { void update(behind); }} /> }]);
  }

  return {
    // In Settings (`arg.settings`) its buttons sit in the page's own toolbar.
    title: () => {
      const current = view();
      return {
        title: "Coding agents", sub: "What each machine can run",
        reactTools: inSettings ? null : <ProviderHeader
          machines={machines()} machine={st.machine} loading={isLoading()}
          checked={checkedLabel(current ? current.checked_at : null, Date.now())}
          updateCount={[...st.updating].filter(key => key.startsWith(`${st.machine} `)).length}
          canUpdate={!!updatable(current || null).length}
          pickMachine={pickMachine} refresh={() => { void load(true); }}
          updateAll={() => { void update(updatable(view() || null)); }} />,
      };
    },
    // Settings opens it at one agent (`arg.agent`), which its left pane picks.
    mount(host, arg) {
      removeWindowPart("settings-page:providers");
      root = host;
      const want = arg;
      if (want?.agent) st.picked = want.agent;
      inSettings = want?.settings === true;
      draw();
      // What the page showed last draws at once; it reads only what it never had.
      if (!machines()) void loadMachines();
      else ensure(st.machine);
    },
    unmount() { removeWindowPart("settings-page:providers"); root = null; },
  };
}
