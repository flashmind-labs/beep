// The Instructions view: the user's rules for every agent Beep runs.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { Ago, Kid, MakeButton, MakeEl } from "../ui/dom.ts";
import { MAX_BYTES, bytes, sizeLabel, versionLine, mineLayer } from "./instructions-state.ts";
import type { VersionRow } from "./instructions-state.ts";
import type { CenterView, InstructionsTab } from "../center/state.ts";
import { emptyPanel, secBlock } from "../ui/parts.ts";

/// The view's tabs.

const TABS: [InstructionsTab, string][] = [["editor", "Editor"], ["history", "History"]];

/// What the window hands `registerInstructions`.
interface InstructionsDeps {
  registerView: (name: "instructions", view: CenterView<"instructions">) => void;
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
  /// The version (`v` and its number) shown.
  shown: string | null;
}

export function registerInstructions(deps: InstructionsDeps) {
  const { registerView, invoke, h, button, ago } = deps;
  const st: InstructionsState = { tab: "editor", newest: undefined, draft: null, note: "", saving: false, error: "", versions: undefined, shown: null };
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

  function setTab(tab: InstructionsTab) {
    st.tab = tab;
    if (tab === "history" && st.versions === undefined) loadVersions();
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
      button("History", "", () => setTab("history")));
    if (dirty()) acts.insertBefore(button("Discard", "quiet", () => { st.draft = null; st.note = ""; draw(); }), acts.children[1]);
    const err = st.error ? h("div", "cc-err", st.error) : null;
    return h("div", "in-cols",
      sec("For all agents", sizeLabel(text()), h("div", "cc-panel in-ed", ver, area, acts, err)),
      h("div", "in-sidecol", layers()));
  }

  /// The instruction sources used by a Beep session.
  function layers() {
    const li = (title: string, note: string, me = false) => h("li", me ? "me" : "", h("b", "", title), h("small", "", note));
    return sec("How agents see it", "instruction sources", h("div", "cc-panel",
      h("ol", "in-layers",
        li("The agent's own rules", "Built into Claude Code, Codex, Gemini CLI or pi"),
        li("Your text in its file", "Your local CLAUDE.md or AGENTS.md, kept as is"),
        li("Your instructions", mineLayer(st.newest), true),
        li("Beep's rules", "Session setup and tool instructions supplied by Beep"),
        li("Project", "The repository's CLAUDE.md or AGENTS.md"),
        li("Task", "The prompt Beep starts the agent with"))));
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
      if (arg?.tab) st.tab = arg.tab;
      st.error = "";
      draw();
      load();
      if (st.tab !== "editor") setTab(st.tab);
    },
  });
}
