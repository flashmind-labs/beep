import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { removeWindowPart, showWindowParts } from "../center/window-root.ts";
import { CustomAgentPrompt, CustomAgentsView } from "./customagents-view.tsx";
// The Custom agents list of a project page: the named agents the coding agent
// harnesses define in the project's folder on one machine, and the user's own
// there.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { el } from "../ui/dom.ts";
import { type AgentsPlace, type CustomAgentRow } from "./customagents-state.ts";
import { reason } from "../ui/format.ts";

interface Listed {
  rows?: CustomAgentRow[];
  error: string;
  loading: boolean;
}

export interface CustomAgentsDeps {
  redraw: () => void;
  /// Start: a coding session with agent `a`, read on `place`.
  start: (a: CustomAgentRow, place: AgentsPlace) => void;
  slot: (host: HTMLElement) => HTMLElement;
}

export function makeCustomAgents(deps: CustomAgentsDeps) {
  const all = new Map<string, Listed>();
  const key = (p: AgentsPlace) => `${p.id}\n${p.folder}`;

  async function load(place: AgentsPlace) {
    const k = key(place);
    let s = all.get(k);
    if (!s) all.set(k, s = { error: "", loading: false });
    if (s.loading) return;
    s.loading = true;

    try {
      s.rows = await invoke<CustomAgentRow[]>("custom_agents", { machineId: place.id, folder: place.folder });
      s.error = "";
    } catch (e) {
      s.error = reason(e);
    } finally {
      s.loading = false;
    }

    deps.redraw();
  }

  /// The rows read on `place`, read the first time it is asked.
  function listed(place: AgentsPlace) {
    const s = all.get(key(place));
    if (!s) load(place);
    return s;
  }

  let mounted: { host: HTMLElement } | null = null;

  function count(place: AgentsPlace | null) { return place ? listed(place)?.rows?.length ?? 0 : 0; }

  function section(place: AgentsPlace | null) {
    const s = place ? listed(place) : null;
    const rows = s?.rows ?? null;
    const message = !place ? "No machine has this project's folder to read its custom agents from."
      : !rows ? s?.error ? `Could not read the custom agents on ${place.name}: ${s.error}` : `Reading the custom agents on ${place.name}…`
      : "No custom agents in .claude/agents, .opencode/agent, .codex/agents, .pi/agents or .agents yet.";
    if (!mounted) {
      const host = el("div");
      host.id = "sec-custom-agents";
      mounted = { host };
    }
    const current = mounted;
    showWindowParts([{ part: "project-custom-agents", host: current.host, content: createElement(CustomAgentsView, {
      rows, message, error: !!s?.error && !rows,
      reload: place ? () => void load(place) : null,
      open: (a: CustomAgentRow) => { if (place) openPrompt(a, place); },
      start: (a: CustomAgentRow) => { if (place) deps.start(a, place); },
    }) }]);
    return deps.slot(current.host);
  }

  function dispose() { if (mounted) removeWindowPart("project-custom-agents"); mounted = null; }

  /// The agent's prompt, read-only, over the window.
  function openPrompt(a: CustomAgentRow, place: AgentsPlace) {
    let promptRoot: Root | null = null;
    const { sheet, close } = openSheet(`Custom agent ${a.name}`, "am-sheet ca-sheet", {
      onClose: () => { promptRoot?.unmount(); promptRoot = null; },
      onKey: e => {
        if (e.key !== "Escape") return;
        e.preventDefault();
        e.stopImmediatePropagation();
        close();
        return true;
      },
    });
    promptRoot = createRoot(sheet);
    flushSync(() => promptRoot?.render(createElement(CustomAgentPrompt, { agent: a, close, start: () => { close(); deps.start(a, place); } })));

  }

  return { count, section, dispose };
}
