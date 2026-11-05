// The Instructions view: the user's rules for every agent Beep runs.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { Ago, Kid, MakeButton, MakeEl } from "../ui/dom.ts";
import { MAX_BYTES, agentName, bytes, sizeLabel, linesLabel, withoutBeepBlock, appended, syncSummary, fileState, behind, versionLine, mineLayer } from "./instructions-state.ts";
import type { AgentFile, MachineAgentFiles, SyncSummary, VersionRow } from "./instructions-state.ts";
import type { ViewName } from "../center/state.ts";
import { plural } from "../ui/format.ts";
import { emptyPanel, note, secBlock } from "../ui/parts.ts";

/// The view's tabs.
type InstructionsTab = "editor" | "import" | "sync" | "history";

const TABS: [InstructionsTab, string][] = [["editor", "Editor"], ["import", "Import"], ["sync", "Sync"], ["history", "History"]];

/// The view as the window registers it.
interface InstructionsView {
  title: () => { title: string; sub: string; tools: Node[] };
  /// `arg.tab` opens on that tab.
  mount(root: HTMLElement, arg?: unknown): void;
}

/// What the window hands `registerInstructions`.
interface InstructionsDeps {
  registerView: (name: ViewName, view: InstructionsView) => void;
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  ago: Ago;
  openSettings: () => void;
}

/// The view's state.
interface InstructionsState {
  tab: InstructionsTab;
  newest: VersionRow | null | undefined;
  draft: string | null;
  note: string;
  saving: boolean;
  error: string;
  versions: VersionRow[] | null | undefined;
  files: MachineAgentFiles[] | null | undefined;
  reading: boolean;
  writing: Set<string>;
  /// The file (machine id and path) or version (`v` and its number) shown.
  shown: string | null;
}

export function registerInstructions(deps: InstructionsDeps) {
  const { registerView, invoke, h, button, ago } = deps;
  const st: InstructionsState = { tab: "editor", newest: undefined, draft: null, note: "", saving: false, error: "", versions: undefined, files: undefined, reading: false, writing: new Set(), shown: null };
  let el: HTMLElement | null = null;

  const fail = (e: unknown) => String(e).replace(/^(bad request|conflict|error):\s*/i, "");
  const text = () => st.draft ?? st.newest?.text ?? "";
  const dirty = () => st.draft != null && st.draft !== (st.newest?.text ?? "");

  async function load() {
    try {
      st.newest = await invoke<VersionRow | null>("instructions_get");
      st.error = "";
    } catch (e) {
      st.newest ??= null;
      st.error = fail(e);
    }
    draw();
  }

  async function save(note?: string) {
    if (st.saving || bytes(text()) > MAX_BYTES) return;
    st.saving = true;
    st.error = "";
    draw();
    try {
      st.newest = await invoke<VersionRow>("instructions_save", { text: text(), base: st.newest?.version ?? null, note: note ?? st.note });
      st.draft = null;
      st.note = "";
      st.versions = undefined;
      st.files = undefined;
    } catch (e) {
      st.error = fail(e);
    } finally {
      st.saving = false;
    }
    draw();
  }

  async function loadVersions() {
    try {
      st.versions = await invoke<VersionRow[]>("instructions_versions");
    } catch (e) {
      st.versions = null;
      st.error = fail(e);
    }
    draw();
  }

  async function readFiles() {
    if (st.reading) return;
    st.reading = true;
    draw();
    try {
      st.files = await invoke<MachineAgentFiles[]>("agent_files_read");
    } catch (e) {
      st.files = null;
      st.error = fail(e);
    } finally {
      st.reading = false;
    }
    draw();
  }

  async function writeFiles(machineId: string) {
    st.writing.add(machineId);
    draw();
    try {
      const after = await invoke<MachineAgentFiles>("agent_files_sync", { machineId });
      st.files = (st.files || []).map(m => (m.machine_id === machineId ? after : m));
    } catch (e) {
      st.files = (st.files || []).map(m => (m.machine_id === machineId ? { ...m, error: fail(e) } : m));
    } finally {
      st.writing.delete(machineId);
    }
    draw();
  }

  function setTab(tab: InstructionsTab) {
    st.tab = tab;
    if (tab === "history" && st.versions === undefined) loadVersions();
    if ((tab === "import" || tab === "sync") && st.files === undefined) readFiles();
    draw();
  }

  function sec(title: string, small: string, ...body: Kid[]) {
    return secBlock(title, small, ...body);
  }

  function tabs() {
    return h("div", "in-tabs", ...TABS.map(([k, label]) => {
      const b = button(label, "tm-tab", () => setTab(k));
      if (st.tab === k) b.setAttribute("aria-current", "true");
      return b;
    }));
  }

  function editor() {
    const area = h("textarea", "");
    area.value = text();
    area.spellcheck = false;
    area.setAttribute("aria-label", "Your instructions");
    area.placeholder = "# How I work\n\n- Write plainly.\n- Ask before deploying anything.";
    const size = h("small", "", sizeLabel(area.value));
    const status = h("span", "sp", st.saving ? "Saving…" : dirty() ? "Not saved" : st.newest ? "Saved" : "");
    const saveBtn = button("Save", "pri", () => save());
    const over = () => bytes(area.value) > MAX_BYTES;
    saveBtn.disabled = st.saving || !dirty() || over();
    area.addEventListener("input", () => {
      st.draft = area.value;
      size.textContent = sizeLabel(area.value);
      size.classList.toggle("bad", over());
      status.textContent = dirty() ? "Not saved" : "Saved";
      saveBtn.disabled = st.saving || !dirty() || over();
    });
    area.addEventListener("keydown", e => { if (e.metaKey && e.key === "s") { e.preventDefault(); save(); } });
    const when = st.newest ? ` · ${ago(st.newest.saved_at)}` : "";
    const ver = h("div", "in-ver", h("span", "", h("b", "", versionLine(st.newest && { ...st.newest, note: "" })), when, st.note ? ` · ${st.note}` : ""), status);
    const acts = h("div", "in-acts", saveBtn,
      button("History", "", () => setTab("history")),
      button("Import from machines", "", () => setTab("import")));
    if (dirty()) acts.insertBefore(button("Discard", "quiet", () => { st.draft = null; st.note = ""; draw(); }), acts.children[1]);
    const err = st.error ? h("div", "cc-err", st.error) : null;
    const summary = st.files ? syncSummary(st.files) : null;
    const files = h("div", "cc-panel cc-row in-side",
      h("span", "t", h("b", "", summary ? `Synced to ${summary.synced} of ${plural(summary.machines, "machine", "machines")}` : "Agent files"),
        summary ? null : h("span", "", "Write your rules into each agent's own file")),
      button("Sync", "", () => setTab("sync")));
    return h("div", "in-cols",
      sec("For all agents", sizeLabel(text()), h("div", "cc-panel in-ed", ver, area, acts, err)),
      h("div", "in-sidecol", layers(summary), sec("Agent files", "", files)));
  }

  /// How agents see it: what a coding agent on a machine reads, first to
  /// last, with this text's place among them.
  function layers(summary: SyncSummary | null) {
    const li = (title: string, note: string, me = false) => h("li", me ? "me" : "", h("b", "", title), h("small", "", note));
    return sec("How agents see it", "first to last", h("div", "cc-panel",
      h("ol", "in-layers",
        li("The agent's own rules", "Built into Claude Code, Codex, Gemini CLI or pi"),
        li("Your text in its file", "The rest of ~/.claude/CLAUDE.md and the like"),
        li("Your instructions", mineLayer(st.newest, summary), true),
        li("Project", "The repository's CLAUDE.md or AGENTS.md"),
        li("Task", "The prompt Beep starts the agent with"))));
  }

  function fileRow(f: AgentFile, acts: Kid[]) {
    return h("div", `in-file${f.text == null ? " miss" : ""}`, h("span", "ag", agentName(f.agent)), h("code", "", f.path), h("span", "r", ...acts));
  }

  function machineBlock(m: MachineAgentFiles, right: Node | null, rows: Node[]) {
    const head = h("b", "", h("span", `ddot${m.error ? " bad" : ""}`), m.machine_name);
    return h("div", "in-mach", head, right || h("span", ""), h("div", "in-files", ...rows));
  }

  /// A machine's line when it has no file rows: why it could not be read,
  /// or that it has no coding agent.
  function missRow(text: string) {
    return h("div", "in-file miss", h("span", "", text));
  }

  /// What the Import and Sync tabs show before the files are read, or
  /// when reading them failed; null once they are there.
  function notRead() {
    if (st.files === undefined || (st.reading && !st.files)) return [emptyPanel(h("b", "", "Reading the agent files on your machines…"))];
    if (st.files === null) return [h("div", "cc-err", st.error || "Could not read the agent files.")];
    return null;
  }

  /// The machines' panel, or that none is online.
  function machinesPanel(machines: HTMLElement[]) {
    return h("div", "cc-panel", ...(machines.length ? machines : [h("div", "cc-empty", "No machine online")]));
  }

  function importTab() {
    const early = notRead();
    if (early) return early;
    const machines = st.files!.map(m => machineBlock(m, null, m.error ? [missRow(m.error)]
      : m.files.length ? m.files.map((f): HTMLElement => {
        const own = withoutBeepBlock(f.text);
        if (f.text == null || !own) return fileRow(f, [f.text == null ? "none" : "only Beep's text"]);
        const note = `Imported from ${agentName(f.agent)} on ${m.machine_name}`;
        return fileRow(f, [linesLabel(own),
          button("Show", "small", () => { st.shown = st.shown === `${m.machine_id}${f.path}` ? null : `${m.machine_id}${f.path}`; draw(); }),
          button("Add to mine", "small", () => { st.draft = appended(text(), own); st.note = note; setTab("editor"); }),
          button("Use instead", "small quiet", () => { st.draft = `${own}\n`; st.note = note; setTab("editor"); })]);
      }).concat(m.files.filter(f => st.shown === `${m.machine_id}${f.path}`).map(f => h("pre", "in-pre", withoutBeepBlock(f.text))))
        : [missRow("No coding agent here")]));
    return [
      sec("Found on your machines", st.reading ? "reading…" : "read just now", machinesPanel(machines)),
      h("div", "in-acts", button("Read again", "", readFiles)),
    ];
  }

  /// The Sync tab's note before the first save: why nothing can be
  /// written yet, as a button to the Editor.
  function nothingSaved() {
    const b = h("button", "cc-note in-go", h("span", "ddot off"),
      h("span", "", h("b", "", "Nothing saved yet."), " Save your rules in Editor, then write them here."),
      h("span", "go", "Open Editor"));
    b.addEventListener("click", () => setTab("editor"));
    return b;
  }

  function syncTab() {
    const early = notRead();
    if (early) return early;
    const saved = !!st.newest;
    const ver = st.newest ? `version ${st.newest.version}` : "nothing saved yet";
    const machines = st.files!.map(m => {
      const busy = st.writing.has(m.machine_id);
      // Before the first save there is nothing to write: no button at all.
      const write = saved ? button(busy ? "Writing…" : "Write to agent files", "small", () => writeFiles(m.machine_id)) : null;
      if (write) write.disabled = busy || !!m.error || !m.files.length || m.files.every(f => f.synced);
      const rows = m.error ? [missRow(m.error)]
        : m.files.length ? m.files.map(f => { const s = fileState(f, saved); return fileRow(f, [h("span", s.cls ? `in-${s.cls}` : "", s.text)]); })
          : [missRow("No coding agent here")];
      return machineBlock(m, write, rows);
    });
    const lead = saved
      ? note("", "Written between ", h("code", "", "<!-- beep:start -->"), " and ", h("code", "", "<!-- beep:end -->"), " in each agent's file. Text outside them is never touched.")
      : nothingSaved();
    const acts = h("div", "in-acts", button("Read again", "", readFiles));
    const todo = saved ? behind(st.files) : [];
    if (todo.length) {
      const busy = todo.some(id => st.writing.has(id));
      const all = button(busy ? "Writing…" : "Write to all machines", "", () => { for (const id of todo) if (!st.writing.has(id)) writeFiles(id); });
      all.disabled = busy;
      acts.prepend(all);
    }
    return [lead, sec("Machines", ver, machinesPanel(machines)), acts];
  }

  function historyTab() {
    if (st.versions === undefined) return [emptyPanel(h("b", "", "Loading the versions…"))];
    if (st.versions === null) return [h("div", "cc-err", st.error || "Could not load the versions.")];
    if (!st.versions.length) return [emptyPanel(h("b", "", "No versions yet"), h("span", "", "Each save makes one."))];
    const rows = st.versions.map(v => {
      const open = st.shown === `v${v.version}`;
      const acts = [button(open ? "Hide" : "Show", "small", () => { st.shown = open ? null : `v${v.version}`; draw(); })];
      if (v.version !== st.newest?.version) {
        acts.push(button("Restore", "small", () => { st.draft = v.text; save(`Restored version ${v.version}`).then(() => setTab("editor")); }));
      }
      return h("div", "in-hist", h("div", "h", h("b", "", versionLine(v)), h("small", "", ago(v.saved_at)), h("span", "acts", ...acts)),
        open ? h("pre", "in-pre", v.text) : null);
    });
    return [sec("History", "newest first", h("div", "cc-panel", ...rows))];
  }

  function draw() {
    if (!el) return;
    const view = h("div", "cc-view in-view", tabs());
    if (st.newest === undefined) view.append(emptyPanel(h("b", "", "Loading your instructions…")));
    else if (st.tab === "editor") view.append(editor());
    else if (st.tab === "import") view.append(...importTab());
    else if (st.tab === "sync") view.append(...syncTab());
    else view.append(...historyTab());
    const old = el.querySelector(":scope > .cc-view");
    const active = document.activeElement;
    const caret = active instanceof HTMLTextAreaElement && el.contains(active) ? active.selectionStart : null;
    if (old) { const top = old.scrollTop; old.replaceWith(view); view.scrollTop = top; } else el.append(view);
    if (caret != null) {
      const area = view.querySelector("textarea");
      area?.focus();
      area?.setSelectionRange(caret, caret);
    }
  }

  registerView("instructions", {
    title: () => ({ title: "Instructions", sub: "Your rules for every agent Beep runs", tools: [button("Settings", "quiet small", () => deps.openSettings())] }),
    mount(root, arg) {
      el = root;
      const tab = arg && typeof arg === "object" && "tab" in arg ? arg.tab : null;
      if (tab) st.tab = tab as InstructionsTab;
      st.error = "";
      draw();
      load();
      if (st.tab !== "editor") setTab(st.tab);
    },
  });
}
