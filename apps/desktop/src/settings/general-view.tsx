import { useEffect } from "react";
import { useStore } from "zustand";
import { skillSync, projectSync, beepSyncOverSyncthing, type SyncSettingKind } from "./skill-sync.ts";
import type { MacSettings } from "./settings.ts";
import { Button, Switch } from "../ui/kit.tsx";

interface Props {
  settings: MacSettings; phone: boolean; busy: ReadonlySet<string>;
  autostart(enabled: boolean): void; notices(enabled: boolean): void; chooseSaves(): void; useDownloads(): void; shortcuts(): void; report(): void;
}
const PREFERENCES: Record<SyncSettingKind, { store: typeof skillSync; id: string; title: string; loading: string; on: string; off: string }> = {
  skills: { store: skillSync, id: "skillSyncSetting", title: "Sync coding agent skills", loading: "Loading skill sync setting...", on: "Keep your coding agent skills in sync across your machines.", off: "Skill syncing is paused on all your machines. Existing files stay in place." },
  projects: { store: projectSync, id: "projectSyncSetting", title: "Project sync", loading: "Loading project sync setting...", on: "Sync projects across all your machines. Turning off keeps existing files in place.", off: "Paused for every project on all machines. Turn on to reconcile your projects." },
  syncthing: { store: beepSyncOverSyncthing, id: "beepSyncOverSyncthingSetting", title: "Always use Beep sync over Syncthing", loading: "Loading Syncthing setting...", on: "Every project, and each one imported later, is excluded from Syncthing.", off: "Each project keeps its own Syncthing setting." },
};
function SyncPreference({ kind }: { kind: SyncSettingKind }) {
  const { store, id, title, loading, on, off } = PREFERENCES[kind];
  const state = useStore(store.state);
  useEffect(() => { void store.load(); }, [store]);
  const note = state.enabled === null ? loading : state.enabled ? on : off;
  return <div id={id} className="dtoggle"><span><b>{title}</b><small>{note}</small>{state.error && <span className="derr" role="alert">{state.error}{state.enabled === null && <> <Button look="quiet" onClick={() => void store.load()}>Retry</Button></>}</span>}</span><Switch label={title} on={state.enabled ?? false} disabled={state.busy || state.enabled === null} flip={() => void store.change()} /></div>;
}
export function GeneralView(p: Props) {
  const s = p.settings;
  return <>{!p.phone && <div className="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the tray after a restart</small></span><Switch id="autostart" label="Launch at login" on={s.autostart} disabled={p.busy.size > 0} flip={() => p.autostart(!s.autostart)} /></div>}
    <div className="dtoggle"><span><b>Coding agent notifications</b><small>{s.agent_notices ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting"}</small></span><Switch id="agentNotices" label="Coding agent notifications" on={s.agent_notices} disabled={p.busy.size > 0} flip={() => p.notices(!s.agent_notices)} /></div>
    <SyncPreference kind="skills" /><SyncPreference kind="projects" /><SyncPreference kind="syncthing" />{!p.phone && <div className="dtoggle"><span><b>Save files to</b><small>{s.save_folder ?? "No folder"}</small></span><span>{s.save_folder_picked && <Button look="quiet" id="useDownloads" disabled={p.busy.size > 0} onClick={p.useDownloads}>Use Downloads</Button>}<Button id="chooseSaves" disabled={p.busy.size > 0} onClick={p.chooseSaves}>Choose…</Button></span></div>}<div className="dtoggle"><span><b>Keyboard shortcuts</b><small>Every shortcut, by where it works</small></span><Button id="openShortcuts" onClick={p.shortcuts}>Show…</Button></div><div className="dtoggle"><span><b>Report a problem</b><small>Collect logs from your machines and send them to us</small></span><Button id="openReport" onClick={p.report}>Report…</Button></div>
  </>;
}
