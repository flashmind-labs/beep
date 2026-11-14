import { useEffect } from "react";
import { useStore } from "zustand";
import { skillSync, projectSync } from "./skill-sync.ts";
import type { MacSettings } from "./settings.ts";
import { Button, Switch } from "../ui/kit.tsx";
import { SettingRow } from "./overview-view.tsx";

interface Props {
  settings: MacSettings; phone: boolean; busy: ReadonlySet<string>;
  autostart(enabled: boolean): void; notices(enabled: boolean): void; chooseSaves(): void; useDownloads(): void; shortcuts(): void; report(): void;
}
function SyncPreference({ kind }: { kind: "skills" | "projects" }) {
  const store = kind === "skills" ? skillSync : projectSync;
  const title = kind === "skills" ? "Sync coding agent skills" : "Project sync";
  const state = useStore(store.state);
  useEffect(() => { void store.load(); }, [store]);
  const note = kind === "projects" ? state.enabled === null ? "Loading project sync setting..." : state.enabled ? "Sync projects across all your machines. Turning off keeps existing files in place." : "Paused for every project on all machines. Turn on to reconcile your projects." : state.enabled === null ? "Loading skill sync setting..." : state.enabled ? "Keep your coding agent skills in sync across your machines." : "Skill syncing is paused on all your machines. Existing files stay in place.";
  return <SettingRow id={kind === "skills" ? "skillSyncSetting" : "projectSyncSetting"} title={title} note={note} extra={state.error && <span className="derr" role="alert">{state.error}{state.enabled === null && <> <Button look="quiet" onClick={() => void store.load()}>Retry</Button></>}</span>}><Switch label={title} on={state.enabled ?? false} disabled={state.busy || state.enabled === null} flip={() => void store.change()} /></SettingRow>;
}
export function GeneralView(p: Props) {
  const s = p.settings;
  return <>{!p.phone && <SettingRow title="Launch at login" note="Keep Beep in the tray after a restart"><Switch id="autostart" label="Launch at login" on={s.autostart} disabled={p.busy.size > 0} flip={() => p.autostart(!s.autostart)} /></SettingRow>}
    <SettingRow title="Coding agent notifications" note={s.agent_notices ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting"}><Switch id="agentNotices" label="Coding agent notifications" on={s.agent_notices} disabled={p.busy.size > 0} flip={() => p.notices(!s.agent_notices)} /></SettingRow>
    <SyncPreference kind="skills" /><SyncPreference kind="projects" />{!p.phone && <SettingRow title="Save files to" note={s.save_folder ?? "No folder"}><span>{s.save_folder_picked && <Button look="quiet" id="useDownloads" disabled={p.busy.size > 0} onClick={p.useDownloads}>Use Downloads</Button>}<Button id="chooseSaves" disabled={p.busy.size > 0} onClick={p.chooseSaves}>Choose…</Button></span></SettingRow>}<SettingRow title="Keyboard shortcuts" note="Every shortcut, by where it works"><Button id="openShortcuts" onClick={p.shortcuts}>Show…</Button></SettingRow><SettingRow title="Report a problem" note="Collect logs from your machines and send them to us"><Button id="openReport" onClick={p.report}>Report…</Button></SettingRow>
  </>;
}
