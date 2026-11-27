// The sections that show a project's machines, running agents and custom agents.
import { formatAgo, plural } from "../../ui/format.ts";
import { whereCount, type Project, type WhereRow } from "../../stores/project/projects-state.ts";
import { plainPath } from "../../parts/project/filesync.ts";
import { makeCustomAgents } from "../../parts/project/customagents.tsx";
import { type AgentsPlace, type CustomAgentRow, agentPrompt, agentsPlace } from "../../stores/project/customagents-state.ts";
import { state, pageFresh } from "../../stores/center/state.ts";
import { showMachineMenu } from "../machines/table.ts";
import { ProjectRunning } from "./running-view.tsx";
import { MachinesSection, type ProjectMachineItem } from "./machines-section-view.tsx";
import { blocked, confirmRemove, removeMenuItems } from "./removemachine.ts";
import { changeFolder, changeFolderItems } from "./changefolder.ts";
import { fsync } from "../../parts/project/sections.ts";
import { terms } from "../../parts/project/stores.ts";
import { pc, projectChanged } from "../../stores/project/barstate.ts";
import { drawProjectComp, fillBar } from "../../parts/project/composer.tsx";
import { missingMachine, gitControls, checkoutItems, readingWhere } from "./loads.tsx";
import { terminalNote, terminalWhere } from "../../stores/terminals/terminal-list.ts";
import { go } from "../../parts/center/nav.ts";
import { type RunningAgent } from "../../stores/agents/running-state.ts";

/// The project's custom agents: the named agents its folder defines for
/// each harness on one machine, and the user's own there.
const cags = makeCustomAgents({ redraw: () => projectChanged.notify(), start: startWithAgent, own: false });

/// "Custom agents", read on this Mac's checkout or else the first machine
/// with one. Named apart from "Agents", the ones running now.
/// How many custom agents the project has, as far as read.
export function customAgentsCount(rows: WhereRow[]) { return cags.count(agentsPlace(rows)); }
export function customAgentsSection(rows: WhereRow[]) { return cags.section(agentsPlace(rows)); }

/// Start on a custom agent: the bar on the machine it was read on, with
/// its harness picked and "Use the <name> agent to " to finish.
function startWithAgent(a: CustomAgentRow, place: AgentsPlace) {
  const name = state.picked;
  if (!name) return;
  if (pc.project !== name) drawProjectComp(name);
  pc.machine = place.id;
  pc.picked.agent = a.harness;
  fillBar(name, agentPrompt(a.name), null);
}

/// Each machine's checkout, sync state and terminals in one flat section.
export function whereSection(p: Project, rows: WhereRow[], known: boolean) {
  if (terms.list(p.name) === undefined) void terms.load(p.name);
  terms.follow(p.name);
  const model = p.imported ? fsync.page(p.name) : null;
  const all = terms.list(p.name);
  const items: ProjectMachineItem[] = [];

  for (const m of rows) {
    const host = state.home?.machines?.find(x => x.id === m.id);
    if (host?.can_host === false && !m.folder) continue;
    const detail = model?.detail?.machines.find(s => s.machine_id === m.id);
    const online = host?.online ?? (detail ? detail.status !== "offline" : m.kind !== "offline");
    const change = m.folder && p.imported && online ? () => changeFolder(p.name, m, m.folder ?? "") : null;
    const remove = m.folder && p.imported ? { why: blocked(p.name, rows, m), run: () => confirmRemove(p.name, m) } : null;
    const missing = m.folder ? null : missingMachine(p, m), git = m.folder ? gitControls(p, m, online) : null;
    const menu = m.folder ? (event: MouseEvent) => {
      const from = m.folder;
      if (!from) return;
      showMachineMenu({ id: m.id, name: m.name, online, can_host: host?.can_host }, () => [
        ...checkoutItems(p, m, from), ...(p.imported ? [...changeFolderItems(p.name, m), ...removeMenuItems(p.name, rows, m)] : []),
      ], event);
    } : null;
    const terminals = m.folder ? all?.filter(t => t.machine_id === m.id) ?? [] : [];
    items.push({ id: m.id, name: m.name, online, folder: m.folder ? plainPath(m.folder) : null, change, remove, missing, git, menu,
      terminals: terminals.map(t => ({ id: t.id, name: t.name, title: `${t.machine_name}: ${t.command} in ${t.cwd}`, sub: `${t.machine_name} · ${terminalWhere(t)}`, note: terminalNote(t, ms => formatAgo(ms, "short")), running: t.state === "running", open: () => { go("pane", { draft: { project: p.name, machine: t.machine_id, terminal: t } }); } })),
      terminalSummary: all === undefined ? "Reading…" : !online ? "Machine offline" : terminals.length ? plural(terminals.length, "terminal", "terminals") : "None running" });
  }
  const note = pageFresh("where", p.name, readingWhere.has(p.name));
  return <section key={p.name} className="pm-section"><MachinesSection sync={fsync} project={p.name} items={items} model={model} imported={p.imported} known={known}
    count={whereCount(rows, false)} fresh={note} /></section>;
}

/// The coding agents running in the project's folders now.
export function agentsSection(running: RunningAgent[]) {
  return <div id="sec-project-agents"><ProjectRunning running={running} /></div>;
}
