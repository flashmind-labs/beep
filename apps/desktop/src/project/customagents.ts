// The Custom agents list of a project page: the named agents the coding agent
// harnesses define in the project's folder on one machine, and the user's own
// there.
import { invoke } from "../ui/tauri.ts";
import { focusBack, sheetShell } from "../ui/parts.ts";
import { harnessEl } from "../agents/harnessicon.ts";
import { dbtn, emptyPanel } from "../ui/parts.ts";
import { agentLabel } from "./projects-state.ts";
import { el } from "../ui/dom.ts";
import { present, reason } from "../addproject/importproject-state.ts";
import { type AgentsPlace, type CustomAgentRow, agentColor, agentMeta } from "./customagents-state.ts";

interface Listed {
  rows?: CustomAgentRow[];
  error: string;
  loading: boolean;
}

export interface CustomAgentsDeps {
  redraw: () => void;
  /// Start: a coding session with agent `a`, read on `place`.
  start: (a: CustomAgentRow, place: AgentsPlace) => void;
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

  function row(a: CustomAgentRow, place: AgentsPlace) {
    if (a.error && !a.description && !a.prompt) {
      return el("div", "pj-task ca-row bad",
        el("span", "t", el("span", "ddot bad"), " ", el("code", "", a.path)),
        el("span", "m", el("span", "cc-err", a.error)));
    }
    const dot = el("span", "ddot ca-dot");
    const color = agentColor(a.color);
    if (color) dot.style.background = color;
    else dot.hidden = true;
    const desc = el("span", "ca-desc", a.description);
    const start = dbtn("Start", "small", e => { e.stopPropagation(); deps.start(a, place); });
    start.title = `A coding session that uses ${a.name}`;
    const r = el("div", "pj-task ca-row",
      el("span", "t", harnessEl(a.harness, "cc-ag ca-ic"), dot, el("b", "", a.name), " ", desc),
      el("span", "m", ...agentMeta(a).map(m => el("span", "", m)), el("code", "", a.path), a.error ? el("span", "cc-err", a.error) : null),
      el("span", "acts", start));
    r.tabIndex = 0;
    r.setAttribute("role", "button");
    r.title = a.description || a.name;
    const open = () => openPrompt(a, place);
    r.addEventListener("click", open);
    r.addEventListener("keydown", e => { if (e.target === r && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); open(); } });
    return r;
  }

  /// The list's body and its count, for the section around it.
  function view(place: AgentsPlace | null): { body: Node; count: number } {
    if (!place) return { body: emptyPanel(el("span", "", "No machine has this project's folder to read its custom agents from.")), count: 0 };
    const s = listed(place);
    if (!s?.rows) {
      const text = s?.error ? `Could not read the custom agents on ${place.name}: ${s.error}` : `Reading the custom agents on ${place.name}…`;
      return { body: emptyPanel(el("span", s?.error ? "cc-err" : "", text)), count: 0 };
    }
    if (!s.rows.length) {
      return { body: emptyPanel(el("span", "", "No custom agents in .claude/agents, .opencode/agent, .codex/agents, .pi/agents or .agents yet.")), count: 0 };
    }
    return { body: el("div", "cc-panel pv-scroll", ...s.rows.map(a => row(a, place))), count: s.rows.length };
  }

  /// The agent's prompt, read-only, over the window.
  function openPrompt(a: CustomAgentRow, place: AgentsPlace) {
    const back = document.activeElement;
    const { root, sheet } = sheetShell(`Custom agent ${a.name}`, "am-sheet ca-sheet");
    const close = () => {
      root.remove();
      document.removeEventListener("keydown", keys, true);
      focusBack(back);
    };
    const keys = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); e.stopImmediatePropagation(); close(); }
    };
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
    root.addEventListener("mousedown", e => { if (e.target === root) close(); });
    document.addEventListener("keydown", keys, true);
    document.body.append(root);
    prompt.tabIndex = 0;
    prompt.focus();
  }

  return { view, reload: load };
}
