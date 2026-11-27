import { flushSync } from "react-dom";
import { CustomAgentPrompt, CustomAgentsView } from "./customagents-view.tsx";
// The Custom agents list of a project page: the named agents the coding agent
// harnesses define in the project's folder on one machine, and the user's own
// there.
import { invoke } from "../../ui/tauri.ts";
import { openSheet } from "../../ui/parts.tsx";
import { type AgentsPlace, type CustomAgentRow } from "../../stores/project/customagents-state.ts";
import { reason } from "../../ui/format.ts";

interface Listed {
  rows?: CustomAgentRow[];
  error: string;
  loading: boolean;
}

interface CustomAgentsDeps {
  redraw: () => void;
  /// Start: a coding session with agent `a`, read on `place`; null offers no Start.
  start: ((a: CustomAgentRow, place: AgentsPlace) => void) | null;
  /// Only the folder's own agents, not the user's.
  own: boolean;
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
      const rows = await invoke<CustomAgentRow[]>("custom_agents", { machineId: place.id, folder: place.folder });
      s.rows = deps.own ? rows.filter(row => row.scope === "project") : rows;
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

  function count(place: AgentsPlace | null) { return place ? listed(place)?.rows?.length ?? 0 : 0; }

  function section(place: AgentsPlace | null) {
    const s = place ? listed(place) : null;
    const rows = s?.rows ?? null;
    const message = !place ? "No machine has this project's folder to read its custom agents from."
      : !rows ? s?.error ? `Could not read the custom agents on ${place.name}: ${s.error}` : `Reading the custom agents on ${place.name}…`
      : "No custom agents in .claude/agents, .opencode/agent, .codex/agents, .pi/agents or .agents yet.";
    return <div id="sec-custom-agents"><CustomAgentsView rows={rows} message={message} error={!!s?.error && !rows}
      reload={place ? () => void load(place) : null}
      open={(a: CustomAgentRow) => { if (place) openPrompt(a, place); }}
      start={deps.start && place ? (a: CustomAgentRow) => deps.start?.(a, place) : null}
    /></div>;
  }

  /// The agent's prompt, read-only, over the window.
  function openPrompt(a: CustomAgentRow, place: AgentsPlace) {
    const { close, render } = openSheet(`Custom agent ${a.name}`, "am-sheet ca-sheet");
    const start = deps.start;
    flushSync(() => render(<CustomAgentPrompt agent={a} close={close} start={start && (() => { close(); start(a, place); })} />));
  }

  return { count, section, reload: load };
}
