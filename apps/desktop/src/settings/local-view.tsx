import { useEffect, useState } from "react";
import { SKINS, setSkin, skinId } from "../ui/skin.ts";
import type { MacControl } from "./settings.ts";

export function AppearanceView() {
  const [selected, setSelected] = useState(skinId);
  useEffect(() => { const changed = (event: StorageEvent) => { if (event.key === "beep.skin") setSelected(skinId()); }; window.addEventListener("storage", changed); return () => window.removeEventListener("storage", changed); }, []);
  return <>{[false, true].map(light => <div className="dskins" key={String(light)}><small>{light ? "Light" : "Dark"}</small><div>{SKINS.filter(skin => !!skin.light === light).map(skin => <button className="dskin" key={skin.id} data-skin-pick={skin.id} aria-pressed={skin.id === selected} onClick={() => { setSkin(skin.id); setSelected(skin.id); }}><i style={{ background: skin.swatch }} />{skin.name}</button>)}</div></div>)}</>;
}
export function ComputerView({ mac, busy, setEnabled, allow }: { mac: MacControl; busy: boolean; setEnabled(enabled: boolean): void; allow(permission: "ScreenRecording" | "Accessibility"): void }) {
  const p = mac.permissions, desktopControl = !navigator.userAgent.includes("Windows");
  const sub = !mac.enabled ? desktopControl ? "Beep can see the screen, click, type and run commands here when you ask" : "Beep can run commands and coding agents here when you ask"
    : mac.status === "failed" ? `Couldn't connect: ${mac.error || "unknown error"}` : mac.status === "connected" ? !desktopControl || p.screen_recording && p.accessibility ? "Connected" : "Connected. Allow both below" : "Connecting…";
  const permission = (key: "ScreenRecording" | "Accessibility", name: string, why: string, ok: boolean) => <div className="dtoggle" key={key}><span><b>{name}</b><small>{why}</small></span>{ok ? <span className="dtag ok">Allowed</span> : <button className="dbtn" data-allow={key} onClick={() => allow(key)}>Allow…</button>}</div>;
  return <><div className="dtoggle"><span><b>Let Beep use this computer</b><small>{sub}</small></span><button type="button" className="dswitch" role="switch" aria-label="Let Beep use this computer" aria-checked={mac.enabled} id="usemac" disabled={busy} onClick={() => { if (!busy) setEnabled(!mac.enabled); }} /></div>
    {mac.enabled && desktopControl && <>{permission("ScreenRecording", "Screen Recording", "To see the screen", p.screen_recording)}{permission("Accessibility", "Accessibility", "To click, type and read app controls", p.accessibility)}</>}
  </>;
}
