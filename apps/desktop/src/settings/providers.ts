// The Coding agents page: per machine, each harness (Claude Code, Codex, pi,
// ...) with its version, sign-in and an on/off switch, and the picked one's
// runtime settings.
import { harnessEl } from "../agents/harnessicon.ts";
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { MakeButton, MakeEl } from "../project/projects-state.ts";
import { checkedLabel, differsLabel, differsOn, draftChanged, draftOf, envProblem, harnessName, hasUpdate, listRows, savedToLabel, settingsInput, summaryLine, toggledInput, updatable, versionLine } from "./providers-state.ts";
import type { HarnessDraft, HarnessesView, HarnessView, MachineSettings, SharedField } from "./providers-state.ts";
import { emptyPanel } from "../ui/parts.ts";
import { flshPanel } from "./flsh.ts";
import { failBanner } from "../ui/failbanner.ts";

/// A machine as `list_machines` gives it; only these fields are read.
interface MachineRow { id: string; name: string; online: boolean }

/// What the window hands `registerProviders`.
export interface ProvidersDeps {
  registerView: (name: string, view: { title: () => { title: string; sub: string; tools: Node[] }; mount(root: HTMLElement, arg?: unknown): void }) => void;
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
  loading: boolean;
  note: string;
  /// Each machine's saved settings, by agent, to say which fields differ
  /// (false from a server too old to list them).
  others: Map<string, MachineSettings[] | false>;
  /// "Save to all hosts" copies the binary path too.
  includeProgram: boolean;
  /// What the last save to all hosts did, by machine and agent.
  savedAll: Map<string, string>;
}

export function registerProviders(deps: ProvidersDeps) {
  const { registerView, invoke, h, button } = deps;
  const st: ProvidersState = { machines: undefined, machine: null, views: new Map(), errors: new Map(), picked: null, drafts: new Map(), updating: new Set(), saving: false, loading: false, note: "", others: new Map(), includeProgram: false, savedAll: new Map() };
  /// The agents whose machines' settings are being read.
  const reading = new Set<string>();
  let el: HTMLElement | null = null;
  /// Drawn in Settings, which lists the agents in its own left pane: the
  /// page shows the one agent it was opened at, under a toolbar of its own.
  let inSettings = false;
  const flsh = flshPanel({ invoke, h, button, redraw: () => draw() });

  const fail = (e: unknown) => String(e).replace(/^(bad request|not found|error):\s*/i, "");
  const view = () => (st.machine ? st.views.get(st.machine) : undefined);
  const key = (agent: string) => `${st.machine} ${agent}`;
  const machineName = () => st.machines?.find(m => m.id === st.machine)?.name ?? "this machine";

  async function loadMachines() {
    try {
      const all = await invoke<MachineRow[]>("list_machines");
      st.machines = all;
      const online = all.filter(m => m.online);
      if (!st.machine || !online.some(m => m.id === st.machine)) st.machine = online[0]?.id ?? null;
    } catch (e) {
      st.machines = null;
      st.note = fail(e);
    }
    deps.redrawTop();
    draw();
    if (st.machine && !st.views.has(st.machine)) load(false);
  }

  async function load(refresh: boolean) {
    const machine = st.machine;
    if (!machine) return;
    st.loading = true;
    draw();
    try {
      const got = await invoke<HarnessesView | null>("agent_harnesses", { machineId: machine, refresh });
      st.views.set(machine, got ?? false);
      st.errors.delete(machine);
    } catch (e) {
      if (!st.views.get(machine)) st.views.set(machine, null);
      st.errors.set(machine, fail(e));
    } finally {
      st.loading = false;
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
    st.saving = true;
    draw();
    try {
      await invoke("harness_settings_save", { machineId: machine, agent: hv.agent, settings: input });
      st.drafts.delete(`${machine} ${hv.agent}`);
      st.savedAll.delete(`${machine} ${hv.agent}`);
      st.others.delete(hv.agent);
      st.errors.delete(machine);
      // Read back what the server saved, without asking npm again.
      await load(false);
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

  /// Saves the form for `hv` on every machine of the user's, online or not.
  async function saveAll(hv: HarnessView, input: ReturnType<typeof settingsInput>) {
    const machine = st.machine;
    if (!machine || st.saving) return;
    st.saving = true;
    draw();
    try {
      const n = await invoke<number>("harness_settings_save_all", { fromMachine: machine, agent: hv.agent, settings: input, includeProgram: st.includeProgram });
      // Drafts on the other machines were of the settings before.
      for (const k of [...st.drafts.keys()]) if (k.endsWith(` ${hv.agent}`)) st.drafts.delete(k);
      st.savedAll.set(`${machine} ${hv.agent}`, savedToLabel(n));
      st.errors.delete(machine);
      st.others.delete(hv.agent);
      // The other machines' lists are read again when shown.
      for (const id of [...st.views.keys()]) if (id !== machine) st.views.delete(id);
      await load(false);
    } catch (e) {
      st.errors.set(machine, fail(e));
    } finally {
      st.saving = false;
    }
    draw();
  }

  /// The switch at the end of a row; its click never picks the row.
  function toggle(hv: HarnessView, on: boolean) {
    const b = h("button", "dswitch");
    b.type = "button";
    b.setAttribute("role", "switch");
    b.setAttribute("aria-checked", String(on));
    b.setAttribute("aria-label", `${harnessName(hv.agent)} on ${machineName()}`);
    b.disabled = st.saving;
    b.addEventListener("click", e => {
      e.stopPropagation();
      save(hv, toggledInput(hv, !on));
    });
    b.addEventListener("keydown", e => e.stopPropagation());
    return b;
  }

  function avatar(agent: string) {
    return harnessEl(agent);
  }

  /// The name, version and update chip on one line; the version gives way
  /// first when the line runs out of room.
  function nameLine(name: string, version: string, latest: string | null) {
    const up = latest ? h("span", "hv-up", "update") : null;
    up?.setAttribute("title", `v${latest} is out`);
    return h("span", "hv-nm", h("b", "", name), version ? h("span", "ver", version) : null, up);
  }

  /// One status line that never wraps; the full text is in its tooltip.
  function statusSmall(text: string, bad: boolean) {
    const s = h("small", bad ? "bad" : "", text);
    s.title = text;
    return s;
  }

  function list(v: HarnessesView) {
    const rows = listRows(v).map(r => {
      const hv = v.harnesses.find(x => x.agent === r.agent)!;
      const name = h("span", "hv-name", nameLine(r.name, r.version ? `v${r.version}` : "", r.update ? hv.latest : null), statusSmall(r.status, r.bad));
      const row = h("div", `hv-it${r.on || !r.switchable ? "" : " off"}`, avatar(r.agent), name, r.switchable ? toggle(hv, r.on) : h("span", ""));
      row.setAttribute("role", "button");
      row.tabIndex = 0;
      row.setAttribute("aria-current", String(r.agent === st.picked));
      const pick = () => { st.picked = r.agent; draw(); };
      row.addEventListener("click", pick);
      row.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(); } });
      return row;
    });
    return h("div", "hv-list", ...rows);
  }

  /// A setting: its label and help on the left, `control` on the right.
  function field(label: string, help: string, control: HTMLElement, names: readonly string[] = []) {
    const note = names.length ? h("span", "hv-diff", differsLabel(names)) : null;
    note?.setAttribute("title", `Not the same on ${names.join(", ")}`);
    return h("div", "hv-f", h("span", "", h("b", "", label), note, h("small", "", help)), control);
  }

  /// The box atop the page in Settings: the agent, its version and status,
  /// and its switch on the machine picked.
  function summary(hv: HarnessView) {
    const s = summaryLine(hv), m = machineName();
    const name = h("span", "hv-name", nameLine(harnessName(hv.agent), hv.installed && hv.version ? `v${hv.version}` : "", hasUpdate(hv) ? hv.latest : null), statusSmall(s.text, s.bad));
    const on = hv.installed && hv.settings.enabled;
    const end = hv.installed ? [h("span", "on", `${on ? "On" : "Off"} for ${m}`), toggle(hv, on)] : [h("span", ""), h("span", "")];
    return h("div", `hv-sum${hv.installed && !on ? " off" : ""}`, avatar(hv.agent), name, ...end);
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

  function statusBlock(hv: HarnessView) {
    const m = machineName(), name = harnessName(hv.agent);
    // In Settings the summary box above already says the status line.
    const row = (...kids: (Node | string | null)[]): Node | null => (inSettings ? null : h("div", "hv-stat", ...kids));
    const nodes = (out: (Node | null)[]) => out.filter((n): n is Node => n !== null);
    const acts = (...kids: Node[]) => h("div", "hv-acts", ...kids);
    const busy = st.updating.has(key(hv.agent));
    if (!hv.installed) {
      const out: (Node | null)[] = [row(`Not installed on ${m}.`)];
      if (hv.install) {
        const install = button(busy ? "Installing…" : `Install on ${m}`, "pri", () => update([hv.agent]));
        install.disabled = busy;
        const copy = button("Copy install command", "quiet", async () => {
          try { await navigator.clipboard.writeText(hv.install!); copy.textContent = "Copied"; } catch (e) { console.warn("copy:", e); }
        });
        out.push(acts(install, copy));
      } else {
        out.push(h("div", "hv-note", "Install it on the machine, then check again."));
      }
      return nodes(out);
    }
    const out: (Node | null)[] = [];
    const s = hv.sign_in;
    if (s.state === "signed_in") {
      const plan = s.plan ? ` · ${s.plan[0].toUpperCase()}${s.plan.slice(1)} plan` : "";
      out.push(row(h("span", "ddot"), s.account ? `Signed in as ${s.account}${plan}` : `Signed in${plan}`));
    } else if (s.state === "beep_token") {
      out.push(row(h("span", "ddot"), `Signs in with the token in Agent accounts (…${s.last4 ?? ""})`),
        acts(button("Agent accounts", "", () => deps.openSettings("agents"))));
    } else if (s.state === "signed_out") {
      const dot = h("span", "ddot");
      dot.style.background = "var(--hot)";
      out.push(row(dot, `Not signed in on ${m}`));
      if (hv.agent === "claude" || hv.agent === "codex") out.push(acts(button("Sign in", "pri", () => deps.openSettings("agents"))));
      else out.push(h("div", "hv-note", `Run ${hv.agent} on ${m} and sign in there.`));
    } else {
      const why = `${name} does not say whether it is signed in. Run it on ${m} to sign in.`;
      out.push(inSettings ? h("div", "hv-note", why) : row(why));
    }
    if (!hv.runnable) out.push(h("div", "hv-note", "Restart beep-node to use it."));
    if (hasUpdate(hv)) {
      const up = button(busy ? "Updating…" : `Update to v${hv.latest}`, "", () => update([hv.agent]));
      up.disabled = busy || !hv.install;
      if (!hv.install) up.title = "Beep knows no way to update it here";
      out.push(acts(up));
    }
    return nodes(out);
  }

  function detail(hv: HarnessView) {
    const k = key(hv.agent);
    const draft = st.drafts.get(k) ?? draftOf(hv);
    const setDraft = (change: Partial<HarnessDraft>) => {
      st.drafts.set(k, { ...draft, ...change });
      Object.assign(draft, change);
      refreshSave();
    };
    const m = machineName(), name = harnessName(hv.agent);
    const box = inSettings
      ? h("div", "hv-d", ...statusBlock(hv))
      : h("div", "hv-d", h("h3", "", avatar(hv.agent), name, h("span", "ver", versionLine(hv))), h("h4", "", "Status"), ...statusBlock(hv));
    if (!hv.installed) return box;

    const homeVar = hv.home_var ?? "Home folder";
    const others = st.others.get(hv.agent);
    if (others === undefined) loadOthers(hv.agent);
    const differs = (f: SharedField) => (others && st.machine ? differsOn(others, st.machine, f) : []);
    box.append(h("h4", "", "Runtime"),
      field("Binary path", `The command Beep starts for ${name} on ${m}.`, input(draft.program, hv.program ?? hv.agent, v => setDraft({ program: v }), "Binary path"), differs("program")),
      field(homeVar, "Its home and config folder.", input(draft.home, hv.home_default ?? "", v => setDraft({ home: v }), homeVar), differs("home")),
      field("Launch arguments", "Added to every session it starts.", input(draft.args, "none", v => setDraft({ args: v }), "Launch arguments"), differs("args")));

    const envBox = h("div", "hv-envs");
    const drawEnv = () => {
      const rows = [...draft.env, { name: "", value: "", saved: false }].map((v, i) => {
        const last = i === draft.env.length;
        const nameIn = input(v.name, "NAME", val => {
          const env = [...draft.env];
          if (last) env.push({ name: val, value: "", saved: false });
          else env[i] = { ...env[i], name: val };
          setDraft({ env });
          if (last) { drawEnv(); envBox.querySelectorAll<HTMLInputElement>("input")[i * 2]?.focus(); }
        }, "Variable name");
        const valIn = input(v.value, v.saved ? "saved; type to replace" : "value", val => {
          const env = [...draft.env];
          if (last) env.push({ name: "", value: val, saved: false });
          else env[i] = { ...env[i], value: val };
          setDraft({ env });
          if (last) { drawEnv(); envBox.querySelectorAll<HTMLInputElement>("input")[i * 2 + 1]?.focus(); }
        }, "Value");
        valIn.type = "password";
        const remove = last ? h("span", "") : button("×", "quiet small", () => {
          setDraft({ env: draft.env.filter((_, j) => j !== i) });
          drawEnv();
        });
        if (!last) remove.setAttribute("aria-label", `Remove ${v.name}`);
        return h("div", "hv-env", nameIn, valIn, remove);
      });
      envBox.replaceChildren(...rows);
    };
    drawEnv();
    box.append(field("Environment", "Encrypted, never shown again. Put keys in Credentials.", envBox, differs("env")));

    const problem = h("span", "derr", "");
    const saveBtn = button(st.saving ? "Saving…" : "Save", "pri", () => {
      if (envProblem(draft.env)) return;
      save(hv, settingsInput(hv.settings.enabled, draft));
    });
    saveBtn.title = `Save for ${m} only`;
    const discard = button("Discard", "quiet", () => { st.drafts.delete(k); draw(); });
    // Save to all hosts: every machine of the user's, offline ones when
    // they next connect; shown once there is another machine to copy to.
    const allBtn = button(st.saving ? "Saving…" : "Save to all hosts", "", () => {
      if (envProblem(draft.env)) return;
      saveAll(hv, settingsInput(hv.settings.enabled, draft));
    });
    allBtn.title = "Save these settings on every machine; offline ones get them when they next connect";
    const withProgram = h("input", "");
    withProgram.type = "checkbox";
    withProgram.checked = st.includeProgram;
    withProgram.addEventListener("change", () => { st.includeProgram = withProgram.checked; });
    const include = h("label", "hv-incl", withProgram, "Include binary path");
    include.title = "Binary paths usually differ between machines";
    const many = !!others && others.length > 1;
    const done = h("span", "hv-ok", st.savedAll.get(k) ?? "");
    const refreshSave = () => {
      const changed = draftChanged(hv, draft);
      const why = envProblem(draft.env);
      problem.textContent = why;
      saveBtn.disabled = st.saving || !changed || !!why;
      allBtn.disabled = st.saving || !!why;
      discard.hidden = !changed;
      if (changed) done.textContent = "";
    };
    refreshSave();
    box.append(h("div", "hv-acts hv-save", saveBtn, many ? allBtn : null, many ? include : null, discard, done, problem));
    // flsh's own settings are the same on every machine and save apart.
    if (hv.agent === "flsh") box.append(...flsh.render());
    return box;
  }

  function body(): Node[] {
    if (st.machines === undefined) return [emptyPanel(h("b", "", "Loading your machines…"))];
    if (st.machines === null) return [h("div", "cc-err", st.note || "Could not load your machines.")];
    if (!st.machine) return [emptyPanel(h("b", "", "No machine online"), h("span", "", "Coding agents show here for machines that are online."))];
    const v = view();
    const err = st.errors.get(st.machine);
    const machine = st.machine;
    const errEl = err ? failBanner(err, () => { st.errors.delete(machine); draw(); }) : null;
    if (v === undefined) return [emptyPanel(h("b", "", `Asking ${machineName()}…`))];
    if (v === false) return [h("div", "cc-err", "The server needs an update before it shows coding agents here.")];
    if (v === null) return [errEl ?? h("div", "cc-err", "Could not read the coding agents.")];
    if (inSettings) {
      // Settings' left pane picks the agent; the page shows that one only.
      const hv = v.harnesses.find(x => x.agent === st.picked);
      const page = hv ? [summary(hv), detail(hv)] : [emptyPanel(h("b", "", `${machineName()} does not report ${harnessName(st.picked ?? "")}`))];
      return errEl ? [errEl, ...page] : page;
    }
    const rows = listRows(v);
    if (!st.picked || !rows.some(r => r.agent === st.picked)) st.picked = rows.find(r => r.on)?.agent ?? rows[0]?.agent ?? null;
    const hv = v.harnesses.find(x => x.agent === st.picked);
    const page = h("div", "hv", list(v), hv ? detail(hv) : h("div", "hv-d"));
    return errEl ? [errEl, page] : [page];
  }

  function draw() {
    if (!el) return;
    const active = document.activeElement;
    const label = active instanceof HTMLInputElement && el.contains(active) ? active.getAttribute("aria-label") : null;
    const at = active instanceof HTMLInputElement ? active.selectionStart : null;
    const viewEl = inSettings
      ? h("div", "cc-view hv-view in-set", h("div", "hv-bar", h("span", "lbl", "Machine"), ...tools(true)), ...body())
      : h("div", "cc-view hv-view", ...body());
    const old = el.querySelector(":scope > .cc-view");
    if (old) old.replaceWith(viewEl); else el.append(viewEl);
    if (label) {
      const again = viewEl.querySelector<HTMLInputElement>(`input[aria-label="${CSS.escape(label)}"]`);
      again?.focus();
      if (again && at != null) again.setSelectionRange(at, at);
    }
  }

  /// The machines as a segmented control whose names never wrap, and the
  /// same as a select, which shows instead when the names do not fit: many
  /// machines or long names here, a narrow toolbar in the CSS.
  function machinePicker() {
    const all = st.machines ?? [];
    const seg = h("span", "hv-seg", ...all.map(m => {
      const b = h("button", "", m.name);
      b.type = "button";
      b.title = m.online ? m.name : `${m.name} (offline)`;
      b.setAttribute("aria-pressed", String(m.id === st.machine));
      b.disabled = !m.online;
      b.addEventListener("click", () => pickMachine(m.id));
      return b;
    }));
    seg.setAttribute("role", "group");
    seg.setAttribute("aria-label", "Machine");
    const sel = h("select", "hv-sel", ...all.map(m => {
      const o = h("option", "", m.online ? m.name : `${m.name} (offline)`);
      o.value = m.id;
      o.disabled = !m.online;
      return o;
    }));
    sel.value = st.machine ?? "";
    sel.setAttribute("aria-label", "Machine");
    sel.addEventListener("change", () => pickMachine(sel.value));
    const letters = all.reduce((n, m) => n + m.name.length, 0);
    return h("span", `hv-pick${all.length > 4 || letters > 48 ? " crowded" : ""}`, seg, sel);
  }

  /// The header buttons; in Settings (`bar`) a spacer puts the check and
  /// Update all at the toolbar's end.
  function tools(bar = false): Node[] {
    const v = view();
    const behind = updatable(v || null);
    const running = [...st.updating].filter(k => k.startsWith(`${st.machine} `)).length;
    const all = button(running ? `Updating ${running}…` : "Update all", "", () => update(behind));
    all.disabled = !!running || !behind.length;
    if (!behind.length && !running) all.title = "Everything is up to date";
    const check = button(st.loading ? "Checking…" : checkedLabel(v ? v.checked_at : null, Date.now()), "quiet", () => load(true));
    check.disabled = st.loading || !st.machine;
    check.title = "Check the versions again";
    return bar ? [machinePicker(), h("span", "sp"), check, all] : [machinePicker(), all, check];
  }

  registerView("providers", {
    // In Settings (`arg.settings`) its buttons sit in the page's own toolbar.
    title: () => ({ title: "Coding agents", sub: "What each machine can run", tools: inSettings ? [] : tools() }),
    // Settings opens it at one agent (`arg.agent`), which its left pane picks.
    mount(root, arg) {
      el = root;
      const want = arg && typeof arg === "object" ? arg as { agent?: string; settings?: boolean } : {};
      if (want.agent) st.picked = want.agent;
      inSettings = want.settings === true;
      draw();
      loadMachines();
    },
  });
}
