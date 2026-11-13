import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { CODING_AGENTS, subKey, type SettingsSub } from "./settings-nav.ts";
import { harnessName } from "./providers-state.ts";
import type { Machine } from "../machines/machines.ts";
import { Button } from "../ui/kit.tsx";

export function CodingAgentOverview({ open }: { open(sub: SettingsSub): void }) {
  return <><div className="dset-h">Coding agents</div><div className="dset-box">{CODING_AGENTS.map(agent => {
    const sub: SettingsSub = { kind: "agent", id: agent };
    return <div className="dset-row" key={agent} data-sub={subKey(sub)} role="button" tabIndex={0} onClick={() => open(sub)} onKeyDown={e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(sub); } }}><HarnessIcon agent={agent} /><span className="tt"><b>{harnessName(agent)}</b><small>Version and updates, on or off, binary path, launch arguments and env vars on each machine</small></span><Icon name="chev" /></div>;
  })}</div></>;
}
function Placeholder() { return <div className="dskel-rows"><div className="dskel" style={{ width: "78%" }} /></div>; }
export function MachinesOverview({ machines, open }: { machines: Machine[] | null | undefined; open(): void }) {
  if (machines === undefined) return <Placeholder />;
  if (!machines) return null;
  return <div className="dtoggle"><span><b>Other machines</b><small>{machines.length ? `${machines.length} linked: ${machines.map(machine => machine.name).join(", ")}` : "None linked yet"}</small></span><Button id="machines" onClick={open}>Show…</Button></div>;
}
export function InstructionsOverview({ open }: { open(): void }) { return <div className="dtoggle"><span><b>Your rules for every agent</b><small>One text for Claude Code, Codex, Gemini CLI and pi, with its versions, written into each agent's file on your machines</small></span><Button id="openInstr" onClick={open}>Open…</Button></div>; }
export function AgentsOverview({ open }: { open(): void }) { return <div className="dtoggle"><span><b>Coding agents on each machine</b><small>Versions and updates, on or off, binary paths, launch arguments and env vars for Claude Code, Codex, pi and the rest</small></span><Button id="openProviders" onClick={open}>Open…</Button></div>; }
export function SkillsOverview({ open }: { open(): void }) { return <div className="dtoggle"><span><b>Skill libraries</b><small>Packs of skills from Git repositories, installed in projects or globally</small></span><Button id="openSkills" onClick={open}>Open…</Button></div>; }
