// The Coding agents page: per machine, each harness (Claude Code, Codex,
// pi, ...) with its version, sign-in and an on/off switch, and the picked
// one's runtime settings. center.ts registers it; providers-state.ts holds
// the words and sums.
import { harnessEl } from "./harnessicon.ts";
import type { invoke as Invoke } from "./tauri.ts";
import type { MakeButton, MakeEl } from "./projects-state.ts";
import { checkedLabel, draftChanged, draftOf, envProblem, harnessName, hasUpdate, listRows, settingsInput, toggledInput, updatable, versionLine } from "./providers-state.ts";
import type { HarnessDraft, HarnessesView, HarnessView } from "./providers-state.ts";
import { emptyPanel } from "./components/parts.ts";
import { flshPanel } from "./flsh.ts";

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

/// The page's state. `views` holds each machine's harnesses once read
/// (null when that failed, false from a server too old); `drafts` the
/// runtime fields being edited, by machine and agent.
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
}

export function registerProviders(deps: ProvidersDeps) {
  const { registerView, invoke, h, button } = deps;
  const st: ProvidersState = { machines: undefined, machine: null, views: new Map(), errors: new Map(), picked: null, drafts: new Map(), updating: new Set(), saving: false, loading: false, note: "" };
  let el: HTMLElement | null = null;
  /// Settings' breadcrumb, told of the agent picked, and the last it was told.
  let onPick: ((agent: string) => void) | null = null;
  let reported: string | null = null;
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
    st.picked = null;
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

  function list(v: HarnessesView) {
    const rows = listRows(v).map(r => {
      const hv = v.harnesses.find(x => x.agent === r.agent)!;
      const name = h("span", "hv-name", h("b", "", r.name),
        r.version ? h("span", "ver", `v${r.version}`) : null,
        r.update ? h("span", "up", "update") : null,
        h("small", r.bad ? "bad" : "", r.status));
      if (r.update) name.querySelector(".up")!.setAttribute("title", "An update is available");
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

  function field(label: string, help: string, input: HTMLInputElement) {
    return h("div", "hv-f", h("span", "", h("b", "", label), h("small", "", help)), input);
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
    const row = (...kids: (Node | string | null)[]) => h("div", "hv-stat", ...kids);
    const acts = (...kids: Node[]) => h("div", "hv-acts", ...kids);
    const busy = st.updating.has(key(hv.agent));
    if (!hv.installed) {
      const out: Node[] = [row(`Not installed on ${m}.`)];
      if (hv.install) {
        const install = button(busy ? "Installing…" : `Install on ${m}`, "pri", () => update([hv.agent]));
        install.disabled = busy;
        const copy = button("Copy install command", "quiet", async () => {
          try { await navigator.clipboard.writeText(hv.install!); copy.textContent = "Copied"; } catch (e) { console.warn("copy:", e); }
        });
        out.push(acts(install, copy));
      } else {
        out.push(h("div", "hv-note", "Beep knows no way to install it here. Install it on the machine, then check again."));
      }
      return out;
    }
    const out: Node[] = [];
    const s = hv.sign_in;
    if (s.state === "signed_in") {
      const plan = s.plan ? ` · ${s.plan[0].toUpperCase()}${s.plan.slice(1)} plan` : "";
      out.push(row(h("span", "ddot"), s.account ? `Signed in as ${s.account}${plan}` : `Signed in${plan}`));
    } else if (s.state === "beep_token") {
      out.push(row(h("span", "ddot"), `Signs in with the token in Agent accounts (…${s.last4 ?? ""})`),
        acts(button("Agent accounts", "", () => deps.openSettings("agents"))));
    } else if (s.state === "signed_out") {
      const dot = h("span", "ddot");
      dot.style.background = "var(--coral)";
      out.push(row(dot, `Not signed in on ${m}`));
      if (hv.agent === "claude" || hv.agent === "codex") out.push(acts(button("Sign in", "pri", () => deps.openSettings("agents"))));
      else out.push(h("div", "hv-note", `Run ${hv.agent} on ${m} and sign in there.`));
    } else {
      out.push(row(`${name} does not say whether it is signed in. Run it on ${m} to sign in.`));
    }
    if (!hv.runnable) out.push(h("div", "hv-note", "Beep cannot start it yet: beep-node finds the agent and its ACP adapter when it starts. Restart beep-node after installing."));
    if (hasUpdate(hv)) {
      const up = button(busy ? "Updating…" : `Update to v${hv.latest}`, "", () => update([hv.agent]));
      up.disabled = busy || !hv.install;
      if (!hv.install) up.title = "Beep knows no way to update it here";
      out.push(acts(up));
    }
    return out;
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
    const head = h("h3", "", avatar(hv.agent), name, h("span", "ver", versionLine(hv)));
    const box = h("div", "hv-d", head, h("h4", "", "Status"), ...statusBlock(hv));
    if (!hv.installed) return box;

    const homeVar = hv.home_var ?? "Home folder";
    box.append(h("h4", "", "Runtime"),
      field("Binary path", `The command Beep starts for ${name} on ${m}.`, input(draft.program, hv.program ?? hv.agent, v => setDraft({ program: v }), "Binary path")),
      field(homeVar, "Its home and config folder.", input(draft.home, hv.home_default ?? "", v => setDraft({ home: v }), homeVar)),
      field("Launch arguments", "Added to every session it starts.", input(draft.args, "none", v => setDraft({ args: v }), "Launch arguments")));

    box.append(h("h4", "", "Environment"));
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
    box.append(envBox, h("div", "hv-note", "Values are kept encrypted and never shown again. Keys belong in Credentials; Beep hands them to the agent itself."));

    const problem = h("span", "derr", "");
    const saveBtn = button(st.saving ? "Saving…" : "Save", "pri", () => {
      if (envProblem(draft.env)) return;
      save(hv, settingsInput(hv.settings.enabled, draft));
    });
    const discard = button("Discard", "quiet", () => { st.drafts.delete(k); draw(); });
    const refreshSave = () => {
      const changed = draftChanged(hv, draft);
      const why = envProblem(draft.env);
      problem.textContent = why;
      saveBtn.disabled = st.saving || !changed || !!why;
      discard.hidden = !changed;
    };
    refreshSave();
    box.append(h("div", "hv-acts", saveBtn, discard, problem));
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
    const errEl = err ? h("div", "cc-err", err) : null;
    if (v === undefined) return [emptyPanel(h("b", "", `Asking ${machineName()}…`))];
    if (v === false) return [h("div", "cc-err", "The server needs an update before it shows coding agents here.")];
    if (v === null) return [errEl ?? h("div", "cc-err", "Could not read the coding agents.")];
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
    const viewEl = h("div", "cc-view hv-view", ...body());
    const old = el.querySelector(":scope > .cc-view");
    if (old) old.replaceWith(viewEl); else el.append(viewEl);
    if (onPick && st.picked && st.picked !== reported) {
      reported = st.picked;
      onPick(st.picked);
    }
    if (label) {
      const again = viewEl.querySelector<HTMLInputElement>(`input[aria-label="${CSS.escape(label)}"]`);
      again?.focus();
      if (again && at != null) again.setSelectionRange(at, at);
    }
  }

  function tools(): Node[] {
    const chips = (st.machines ?? []).map(m => {
      const b = button(m.name, "dchip", () => pickMachine(m.id));
      b.className = "dchip";
      b.setAttribute("aria-pressed", String(m.id === st.machine));
      b.disabled = !m.online;
      if (!m.online) b.title = "offline";
      return b;
    });
    const v = view();
    const behind = updatable(v || null);
    const running = [...st.updating].filter(k => k.startsWith(`${st.machine} `)).length;
    const all = button(running ? `Updating ${running}…` : "Update all", "", () => update(behind));
    all.disabled = !!running || !behind.length;
    if (!behind.length && !running) all.title = "Everything is up to date";
    const check = button(st.loading ? "Checking…" : checkedLabel(v ? v.checked_at : null, Date.now()), "quiet", () => load(true));
    check.disabled = st.loading || !st.machine;
    check.title = "Check the versions again";
    return [h("span", "hv-mach", ...chips), all, check];
  }

  registerView("providers", {
    title: () => ({ title: "Coding agents", sub: "What each machine can run", tools: tools() }),
    // Settings opens it at one agent (`arg.agent`) and follows the one
    // picked (`arg.onPick`), for its breadcrumb.
    mount(root, arg) {
      el = root;
      const want = arg && typeof arg === "object" ? arg as { agent?: string; onPick?: (agent: string) => void } : {};
      if (want.agent) st.picked = want.agent;
      onPick = want.onPick ?? null;
      reported = st.picked;
      draw();
      loadMachines();
    },
  });
}
