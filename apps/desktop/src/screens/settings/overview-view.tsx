import { useEffect, useRef, type ReactNode } from "react";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { HarnessIcon, Icon } from "../../ui/icons.tsx";
import { CODING_AGENTS, subKey, type SettingsSub } from "../../stores/settings/settings-nav.ts";
import { onRowKey, SkeletonRows } from "../../ui/kit.tsx";
import { harnessName } from "../../stores/settings/providers-state.ts";
import type { Machine } from "../machines/machines.tsx";
import { Button } from "../../ui/kit.tsx";

/// The row Settings search picked: it scrolls into view, takes focus and
/// shows marked for a moment.
export const pointedSetting = createStore<{ title: string | null }>()(() => ({ title: null }));

/// A settings row: title and note on the left, its control on the right. `extra` goes under the note.
export function SettingRow({ title, note, id, extra = null, children }: { title: ReactNode; note: ReactNode; id?: string; extra?: ReactNode; children: ReactNode }) {
  const pointed = useStore(pointedSetting, state => state.title !== null && state.title === title);
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!pointed) return;
    row.current?.scrollIntoView({ block: "center" });
    row.current?.querySelector<HTMLElement>("button:not([disabled]), input:not([disabled])")?.focus({ preventScroll: true });
    const done = setTimeout(() => pointedSetting.setState({ title: null }), 1600);
    return () => clearTimeout(done);
  }, [pointed]);

  return <div ref={row} id={id} className={pointed ? "dtoggle pointed" : "dtoggle"}><span><b>{title}</b><small>{note}</small>{extra}</span>{children}</div>;
}

export function CodingAgentOverview({ open }: { open(sub: SettingsSub): void }) {
  return <><div className="dlead">Versions, binary paths, launch arguments and environment variables on each machine.</div><div className="dset-box">{CODING_AGENTS.map(agent => {
    const sub: SettingsSub = { kind: "agent", id: agent };
    return <div className="dset-row" key={agent} data-sub={subKey(sub)} role="button" tabIndex={0} onClick={() => open(sub)} onKeyDown={onRowKey(() => open(sub))}><HarnessIcon agent={agent} /><span className="tt"><b>{harnessName(agent)}</b><small>Version and updates, on or off, binary path, launch arguments and env vars</small></span><Icon name="chev" /></div>;
  })}</div></>;
}
function Placeholder() { return <SkeletonRows widths={[78]} />; }
export function MachinesOverview({ machines, open }: { machines: Machine[] | null | undefined; open(): void }) {
  if (machines === undefined) return <Placeholder />;
  if (!machines) return null;
  return <SettingRow title="Other machines" note={machines.length ? `${machines.length} linked: ${machines.map(machine => machine.name).join(", ")}` : "None linked yet"}><Button id="machines" onClick={open}>Show…</Button></SettingRow>;
}
export function InstructionsOverview({ open }: { open(): void }) { return <SettingRow title="Your rules for every agent" note="One text for Claude Code, Codex and pi, with its versions, written into each agent's file on your machines"><Button id="openInstr" onClick={open}>Open…</Button></SettingRow>; }
export function AgentsOverview({ open }: { open(): void }) { return <SettingRow title="Coding agents on each machine" note="Versions and updates, on or off, binary paths, launch arguments and env vars for Claude Code, Codex, pi and the rest"><Button id="openProviders" onClick={open}>Open…</Button></SettingRow>; }
export function SkillsOverview({ open }: { open(): void }) { return <SettingRow title="Skill libraries" note="Packs of skills from Git repositories, installed in projects or globally"><Button id="openSkills" onClick={open}>Open…</Button></SettingRow>; }
