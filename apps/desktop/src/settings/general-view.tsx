import { useEffect } from "react";
import { useStore } from "zustand";
import { skillSync, projectSync } from "./skill-sync.ts";
import type { MacSettings } from "./settings.ts";

interface Props {
  settings: MacSettings; phone: boolean; busy: ReadonlySet<string>;
  autostart(enabled: boolean): void; notices(enabled: boolean): void; shortcuts(): void; report(): void;
}
function SyncPreference({ kind }: { kind: "skills" | "projects" }) {
  const store = kind === "skills" ? skillSync : projectSync;
  const title = kind === "skills" ? "Sync coding agent skills" : "Project sync";
  const state = useStore(store.state);
  useEffect(() => { void store.load(); }, [store]);
  const note = kind === "projects" ? state.enabled === null ? "Loading project sync setting..." : state.enabled ? "Sync projects across all your machines. Turning off keeps existing files in place." : "Paused for every project on all machines. Turn on to reconcile your projects." : state.enabled === null ? "Loading skill sync setting..." : state.enabled ? "Keep your coding agent skills in sync across your machines." : "Skill syncing is paused on all your machines. Existing files stay in place.";
  return <div id={kind === "skills" ? "skillSyncSetting" : "projectSyncSetting"} className="dtoggle"><span><b>{title}</b><small>{note}</small>{state.error && <span className="derr" role="alert">{state.error}{state.enabled === null && <> <button className="dbtn quiet" onClick={() => void store.load()}>Retry</button></>}</span>}</span><button type="button" className="dswitch" role="switch" aria-label={title} aria-checked={state.enabled ?? false} disabled={state.busy || state.enabled === null} onClick={() => void store.change()} /></div>;
}
export function GeneralView(p: Props) {
  const s = p.settings;
  return <>{!p.phone && <div className="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the tray after a restart</small></span><button type="button" className="dswitch" role="switch" id="autostart" aria-label="Launch at login" aria-checked={s.autostart} disabled={p.busy.size > 0} onClick={() => p.autostart(!s.autostart)} /></div>}
    <div className="dtoggle"><span><b>Coding agent notifications</b><small>{s.agent_notices ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting"}</small></span><button type="button" className="dswitch" role="switch" id="agentNotices" aria-label="Coding agent notifications" aria-checked={s.agent_notices} disabled={p.busy.size > 0} onClick={() => p.notices(!s.agent_notices)} /></div>
    <SyncPreference kind="skills" /><SyncPreference kind="projects" /><div className="dtoggle"><span><b>Keyboard shortcuts</b><small>Every shortcut, by where it works</small></span><button className="dbtn" id="openShortcuts" onClick={p.shortcuts}>Show…</button></div><div className="dtoggle"><span><b>Report a problem</b><small>Collect logs from your machines and send them to us</small></span><button className="dbtn" id="openReport" onClick={p.report}>Report…</button></div>
  </>;
}
