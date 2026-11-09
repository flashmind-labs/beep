import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { CustomAgentsView } from "./customagents-view.tsx";
// The Custom agents list of a project page: the named agents the coding agent
// harnesses define in the project's folder on one machine, and the user's own
// there.
import { invoke } from "../ui/tauri.ts";
import { harnessEl } from "../ui/harnessicon.ts";
import { dbtn, openSheet } from "../ui/parts.ts";
import { agentLabel } from "./projects-state.ts";
import { el, present } from "../ui/dom.ts";
import { type AgentsPlace, type CustomAgentRow, agentMeta } from "./customagents-state.ts";
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

  let mounted: { host: HTMLElement; root: Root } | null = null;

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
      mounted = { host, root: createRoot(host) };
    }
    const current = mounted;
    flushSync(() => current.root.render(createElement(CustomAgentsView, {
      rows, message, error: !!s?.error && !rows,
      reload: place ? () => void load(place) : null,
      open: (a: CustomAgentRow) => { if (place) openPrompt(a, place); },
      start: (a: CustomAgentRow) => { if (place) deps.start(a, place); },
    })));
    return deps.slot(current.host);
  }

  function dispose() { mounted?.root.unmount(); mounted = null; }

  /// The agent's prompt, read-only, over the window.
  function openPrompt(a: CustomAgentRow, place: AgentsPlace) {
    const { sheet, close } = openSheet(`Custom agent ${a.name}`, "am-sheet ca-sheet", {
      onKey: e => {
        if (e.key !== "Escape") return;
        e.preventDefault();
        e.stopImmediatePropagation();
        close();
        return true;
      },
    });
    const head = el("b", "", harnessEl(a.harness, "cc-ag ca-ic"), a.name);
    const sub = [agentLabel(a.harness), ...agentMeta(a), a.path].join(" · ");
    const prompt = el("pre", "ca-prompt", a.prompt || "No prompt.");
    sheet.append(...present([
      el("div", "am-head", head, el("kbd", "", "esc")),
      el("div", "am-sub", sub),
      a.description ? el("div", "ca-about", a.description) : null,
      a.tools.length ? el("div", "am-sub", `Tools: ${a.tools.join(", ")}`) : null,
      a.error ? el("div", "am-msg bad", a.error) : null,
      prompt,
      a.prompt_cut ? el("div", "am-sub", "The prompt goes on; open the file to read the rest.") : null,
      el("div", "gp-foot", dbtn("Close", "quiet", close), dbtn("Start", "pri", () => { close(); deps.start(a, place); })),
    ]));
    prompt.tabIndex = 0;
    prompt.focus();
  }

  return { count, section, dispose };
}
