// The Instructions view: the user's rules for every agent Beep runs.
import type { invoke as Invoke } from "../../ui/tauri.ts";
import { removeWindowPart, showWindowParts } from "../../parts/center/window-root.tsx";
import type { Ago } from "../../ui/format.ts";
import { MAX_BYTES, bytes, withSuggestion } from "../../stores/settings/instructions-state.ts";
import type { VersionRow } from "../../stores/settings/instructions-state.ts";
import type { CenterView, InstructionsTab } from "../../stores/center/state.ts";
import { Button } from "../../ui/kit.tsx";
import { InstructionsView, type InstructionsState } from "./instructions-view.tsx";

/// What the window hands `instructionsPage`.
interface InstructionsDeps {
  invoke: typeof Invoke;
  ago: Ago;
  openSettings: () => void;
}

export function instructionsPage(deps: InstructionsDeps): CenterView<"instructions"> {
  const { invoke, ago } = deps;
  const st: InstructionsState = { tab: "editor", newest: undefined, draft: null, note: "", saving: false, error: "", versions: undefined, shown: null };
  let host: HTMLElement | null = null;

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
    if (!host) return;
    showWindowParts([{ part: "settings-page:instructions", host, content: <InstructionsView
      state={st} ago={ago} setTab={setTab}
      setDraft={value => { st.draft = value; draw(); }}
      save={() => { void save(); }}
      discard={() => { st.draft = null; st.note = ""; draw(); }}
      showVersion={version => { st.shown = st.shown === `v${version}` ? null : `v${version}`; draw(); }}
      addSuggestion={suggestion => { st.draft = withSuggestion(text(), suggestion); draw(); }}
      restoreVersion={version => { st.draft = version.text; void save(`Restored version ${version.version}`).then(() => setTab("editor")); }} /> }]);
  }

  function unmount() { removeWindowPart("settings-page:instructions"); host = null; }

  return {
    title: () => ({ title: "Instructions", sub: "Your rules for every agent Beep runs", reactTools: <Button look="quiet" small onClick={() => deps.openSettings()}>Settings</Button> }),
    mount(container, arg) {
      unmount();
      host = container;
      if (arg?.tab) st.tab = arg.tab;
      st.error = "";
      draw();
      load();
      if (st.tab !== "editor") setTab(st.tab);
    },
    unmount,
  };
}
