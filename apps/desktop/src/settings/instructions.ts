// The Instructions view: the user's rules for every agent Beep runs.
import type { invoke as Invoke } from "../ui/tauri.ts";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import type { Ago, MakeButton, MakeEl } from "../ui/dom.ts";
import { MAX_BYTES, bytes } from "./instructions-state.ts";
import type { VersionRow } from "./instructions-state.ts";
import type { CenterView, InstructionsTab } from "../center/state.ts";
import { InstructionsView, type InstructionsState } from "./instructions-view.tsx";

/// What the window hands `registerInstructions`.
interface InstructionsDeps {
  registerView: (name: "instructions", view: CenterView<"instructions">) => void;
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  ago: Ago;
  openSettings: () => void;
}

export function registerInstructions(deps: InstructionsDeps) {
  const { registerView, invoke, button, ago } = deps;
  const st: InstructionsState = { tab: "editor", newest: undefined, draft: null, note: "", saving: false, error: "", versions: undefined, shown: null };
  let root: Root | null = null;

  const fail = (e: unknown) => String(e).replace(/^(bad request|conflict|error):\s*/i, "");
  const text = () => st.draft ?? st.newest?.text ?? "";

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
    const submitted = text();
    st.saving = true;
    st.error = "";
    draw();

    try {
      st.newest = await invoke<VersionRow>("instructions_save", { text: submitted, base: st.newest?.version ?? null, note: note ?? st.note });
      if (st.draft === submitted) { st.draft = null; st.note = ""; }
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

  function draw() {
    if (!root) return;
    flushSync(() => root?.render(createElement(InstructionsView, {
      state: st, ago, setTab,
      setDraft: value => { st.draft = value; draw(); },
      save: () => { void save(); },
      discard: () => { st.draft = null; st.note = ""; draw(); },
      showVersion: version => { st.shown = st.shown === `v${version}` ? null : `v${version}`; draw(); },
      restoreVersion: version => { st.draft = version.text; void save(`Restored version ${version.version}`).then(() => setTab("editor")); },
    })));
  }

  function unmount() { root?.unmount(); root = null; }
  function reactRoot(host: HTMLElement) { root = createRoot(host); }

  registerView("instructions", {
    title: () => ({ title: "Instructions", sub: "Your rules for every agent Beep runs", tools: [button("Settings", "quiet small", () => deps.openSettings())] }),
    mount(root, arg) {
      unmount();
      root.replaceChildren();
      reactRoot(root);
      if (arg?.tab) st.tab = arg.tab;
      st.error = "";
      draw();
      load();
      if (st.tab !== "editor") setTab(st.tab);
    },
    unmount,
  });
}
