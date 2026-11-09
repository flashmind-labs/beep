import { useEffect, useSyncExternalStore } from "react";
import { skillSync, projectSync } from "./skill-sync.ts";
import type { MacSettings, NoticeVoice } from "./settings.ts";

interface Props {
  settings: MacSettings; phone: boolean; server: boolean; busy: ReadonlySet<string>;
  autostart(enabled: boolean): void; voice(choice: NoticeVoice): void; pause(enabled: boolean): void; notices(enabled: boolean): void; shortcuts(): void; report(): void;
}
const voices: [NoticeVoice, string][] = [["ask", "Ask first"], ["aloud", "Read aloud right away"], ["off", "Off"]];
const voiceNotes: Record<NoticeVoice, string> = { ask: "Shown by the orb with Play when you are here and not on a call", aloud: "Read aloud when you are here and not on a call", off: "Notification only" };
function SyncPreference({ kind }: { kind: "skills" | "projects" }) {
  const store = kind === "skills" ? skillSync : projectSync;
  const title = kind === "skills" ? "Sync coding agent skills" : "Project sync";
  const state = useSyncExternalStore(store.subscribe, store.snapshot);
  useEffect(() => { void store.load(); }, [store]);
  const note = kind === "projects" ? state.enabled === null ? "Loading project sync setting..." : state.enabled ? "Sync projects across all your machines. Turning off keeps existing files in place." : "Paused for every project on all machines. Turn on to reconcile your projects." : state.enabled === null ? "Loading skill sync setting..." : state.enabled ? "Keep your coding agent skills in sync across your machines." : "Skill syncing is paused on all your machines. Existing files stay in place.";
  return <div id={kind === "skills" ? "skillSyncSetting" : "projectSyncSetting"} className="dtoggle"><span><b>{title}</b><small>{note}</small>{state.error && <span className="derr" role="alert">{state.error}{state.enabled === null && <> <button className="dbtn quiet" onClick={() => void store.load()}>Retry</button></>}</span>}</span><button type="button" className="dswitch" role="switch" aria-label={title} aria-checked={state.enabled ?? false} disabled={state.busy || state.enabled === null} onClick={() => void store.change()} /></div>;
}
export function GeneralView(p: Props) {
  const s = p.settings;
  return <>{!p.phone && <div className="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the tray after a restart</small></span><button type="button" className="dswitch" role="switch" id="autostart" aria-label="Launch at login" aria-checked={s.autostart} disabled={p.busy.size > 0} onClick={() => p.autostart(!s.autostart)} /></div>}
    {p.server && <><div className="dtoggle"><span><b>Voice notes</b><small>{voiceNotes[s.notice_voice] ?? ""}</small></span></div><div className="dchips dgap" role="radiogroup" aria-label="Voice notes">{voices.map(([key, label]) => <button key={key} className="dchip" role="radio" aria-checked={s.notice_voice === key} aria-pressed={s.notice_voice === key} data-vn={key} disabled={p.busy.size > 0} onClick={() => p.voice(key)}>{label}</button>)}</div>
      <div className="dtoggle"><span><b>When I stop talking</b><small>{s.send_on_pause ? "Sends after a short pause. Saying “send” sends at once." : "Pauses never send. Say “send” or “stop” when you are done."}</small></span></div><div className="dchips dgap" role="radiogroup" aria-label="When I stop talking"><button className="dchip" role="radio" aria-checked={s.send_on_pause} aria-pressed={s.send_on_pause} data-pause="1" disabled={p.busy.size > 0} onClick={() => p.pause(true)}>Send when I stop talking</button><button className="dchip" role="radio" aria-checked={!s.send_on_pause} aria-pressed={!s.send_on_pause} data-pause="0" disabled={p.busy.size > 0} onClick={() => p.pause(false)}>Wait until I say “send”</button></div></>}
    <div className="dtoggle"><span><b>Coding agent notifications</b><small>{s.agent_notices ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting"}</small></span><button type="button" className="dswitch" role="switch" id="agentNotices" aria-label="Coding agent notifications" aria-checked={s.agent_notices} disabled={p.busy.size > 0} onClick={() => p.notices(!s.agent_notices)} /></div>
    <SyncPreference kind="skills" /><SyncPreference kind="projects" /><div className="dtoggle"><span><b>Keyboard shortcuts</b><small>Every shortcut, by where it works</small></span><button className="dbtn" id="openShortcuts" onClick={p.shortcuts}>Show…</button></div><div className="dtoggle"><span><b>Report a problem</b><small>Collect logs from your machines and send them to us</small></span><button className="dbtn" id="openReport" onClick={p.report}>Report…</button></div>
  </>;
}
