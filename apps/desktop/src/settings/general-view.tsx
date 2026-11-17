import { useEffect } from "react";
import { useStore } from "zustand";
import { skillSync, projectSync, type SyncSettingKind } from "./skill-sync.ts";
import type { MacSettings } from "./settings.tsx";
import { Button, Switch } from "../ui/kit.tsx";
import { SettingRow } from "./overview-view.tsx";
import { useBuildCapabilities } from "../ui/tauri.ts";

interface Props {
  settings: MacSettings; phone: boolean; busy: ReadonlySet<string>;
  autostart(enabled: boolean): void; notices(enabled: boolean): void; developer(enabled: boolean): void; chooseSaves(): void; useDownloads(): void; shortcuts(): void; report(): void;
}
const PREFERENCES: Record<SyncSettingKind, { store: typeof skillSync; id: string; title: string; loading: string; on: string; off: string }> = {
  skills: { store: skillSync, id: "skillSyncSetting", title: "Sync coding agent skills", loading: "Loading skill sync setting...", on: "Keep your coding agent skills in sync across your machines.", off: "Skill syncing is paused on all your machines. Existing files stay in place." },
  projects: { store: projectSync, id: "projectSyncSetting", title: "Project sync", loading: "Loading project sync setting...", on: "Sync projects across all your machines, in place of Syncthing. Turning off keeps existing files in place.", off: "Paused for every project on all machines. Turn on to reconcile your projects." },
};
function SyncPreference({ kind }: { kind: SyncSettingKind }) {
  const { store, id, title, loading, on, off } = PREFERENCES[kind];
  const state = useStore(store.state);
  useEffect(() => { void store.load(); }, [store]);
  const note = state.enabled === null ? loading : state.enabled ? on : off;
  return <SettingRow id={id} title={title} note={note} extra={state.error && <span className="derr" role="alert">{state.error}{state.enabled === null && <> <Button look="quiet" onClick={() => void store.load()}>Retry</Button></>}</span>}><Switch label={title} on={state.enabled ?? false} disabled={state.busy || state.enabled === null} flip={() => void store.change()} /></SettingRow>;
}
export function GeneralView(p: Props) {
  const s = p.settings;
  const capabilities = useBuildCapabilities();
  return <>{!p.phone && !capabilities.local_host && <SettingRow title="Lean build" note="Connect another machine to run agents, terminals and projects. The full build can also run work on this computer.">{null}</SettingRow>}
    {!p.phone && capabilities.autostart && <SettingRow title="Launch at login" note="Keep Beep in the tray after a restart"><Switch id="autostart" label="Launch at login" on={s.autostart} disabled={p.busy.size > 0} flip={() => p.autostart(!s.autostart)} /></SettingRow>}
    <SettingRow title="Coding agent notifications" note={s.agent_notices ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting"}><Switch id="agentNotices" label="Coding agent notifications" on={s.agent_notices} disabled={p.busy.size > 0} flip={() => p.notices(!s.agent_notices)} /></SettingRow>
    {/* A phone hosts no files and has no keyboard: these belong to the desktops. */}
    {!p.phone && <><SyncPreference kind="skills" /><SyncPreference kind="projects" /><SettingRow title="Save files to" note={s.save_folder ?? "No folder"}><span>{s.save_folder_picked && <Button look="quiet" id="useDownloads" disabled={p.busy.size > 0} onClick={p.useDownloads}>Use Downloads</Button>}{capabilities.local_host && <Button id="chooseSaves" disabled={p.busy.size > 0} onClick={p.chooseSaves}>Choose…</Button>}</span></SettingRow><SettingRow title="Keyboard shortcuts" note="Every shortcut, by where it works"><Button id="openShortcuts" onClick={p.shortcuts}>Show…</Button></SettingRow>
      <SettingRow title="Developer mode" note={s.developer ? "Updates to every release, patches included" : "Updates to minor releases only"}><Switch id="developerMode" label="Developer mode" on={s.developer} disabled={p.busy.size > 0} flip={() => p.developer(!s.developer)} /></SettingRow></>}<SettingRow title="Report a problem" note="Collect logs from your machines and send them to us"><Button id="openReport" onClick={p.report}>Report…</Button></SettingRow>
  </>;
}
