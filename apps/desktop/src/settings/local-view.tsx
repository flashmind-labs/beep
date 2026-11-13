import { lazy, Suspense, useEffect, useState } from "react";
import { SKINS, frontLayout, setFrontLayout, setSkin, skinId, type FrontLayout, type FrontPage } from "../ui/skin.ts";
import { useStore } from "zustand";
import { conversationAppearance, setConversationAppearance } from "../ui/conversation-appearance.ts";
import type { MacControl } from "./settings.ts";
import { Switch } from "../ui/kit.tsx";

const LAYOUTS: readonly { id: FrontLayout; label: string }[] = [{ id: "board", label: "Board" }, { id: "list", label: "List" }];
const FRONT_PAGES: readonly { page: FrontPage; name: string; note: string }[] = [
  { page: "home", name: "Home", note: "Lanes of cards, or sections of rows" },
  { page: "project", name: "Projects", note: "A project's front page" },
];
// React Aria sets up global listeners on load, so it loads with the view that
// shows it rather than with Settings.
const LayoutSegmented = lazy(() => import("../ui/aria-kit.tsx").then(({ Segmented }) => ({ default: Segmented<FrontLayout> })));

function LayoutPick({ page, name, note }: { page: FrontPage; name: string; note: string }) {
  const [layout, setLayout] = useState(() => frontLayout(page));
  return <div className="dtoggle"><span><b>{name}</b><small>{note}</small></span>
    <Suspense fallback={null}><LayoutSegmented label={`${name} layout`} options={LAYOUTS} value={layout} onChange={next => { setFrontLayout(page, next); setLayout(next); }}/></Suspense></div>;
}

export function AppearanceView() {
  const [selected, setSelected] = useState(skinId);
  const appearance = useStore(conversationAppearance);
  useEffect(() => { const changed = (event: StorageEvent) => { if (event.key === "beep.skin") setSelected(skinId()); }; window.addEventListener("storage", changed); return () => window.removeEventListener("storage", changed); }, []);
  return <>{[false, true].map(light => <div className="dskins" key={String(light)}><small>{light ? "Light" : "Dark"}</small><div>{SKINS.filter(skin => !!skin.light === light).map(skin => <button className="dskin" key={skin.id} data-skin-pick={skin.id} aria-pressed={skin.id === selected} onClick={() => { setSkin(skin.id); setSelected(skin.id); }}><i style={{ background: skin.swatch }} />{skin.name}</button>)}</div></div>)}{FRONT_PAGES.map(front => <LayoutPick key={front.page} {...front}/>)}
    <div className="dtoggle"><span><b>Conversation scrollbar</b><small>Show the scrollbar beside agent conversations</small></span><Switch label="Conversation scrollbar" on={appearance.scrollbar} flip={() => setConversationAppearance("scrollbar", !appearance.scrollbar)} /></div>
    <div className="dtoggle"><span><b>Prompt marks</b><small>Mark your messages in the conversation scrollbar</small></span><Switch label="Prompt marks" on={appearance.promptMarks} flip={() => setConversationAppearance("promptMarks", !appearance.promptMarks)} /></div>
  </>;
}
export function ComputerView({ mac, busy, setEnabled, allow }: { mac: MacControl; busy: boolean; setEnabled(enabled: boolean): void; allow(permission: "ScreenRecording" | "Accessibility"): void }) {
  const p = mac.permissions, desktopControl = !navigator.userAgent.includes("Windows");
  const sub = !mac.enabled ? desktopControl ? "Beep can see the screen, click, type and run commands here when you ask" : "Beep can run commands and coding agents here when you ask"
    : mac.status === "failed" ? `Couldn't connect: ${mac.error || "unknown error"}` : mac.status === "connected" ? !desktopControl || p.screen_recording && p.accessibility ? "Connected" : "Connected. Allow both below" : "Connecting…";
  const permission = (key: "ScreenRecording" | "Accessibility", name: string, why: string, ok: boolean) => <div className="dtoggle" key={key}><span><b>{name}</b><small>{why}</small></span>{ok ? <span className="dtag ok">Allowed</span> : <button className="dbtn" data-allow={key} onClick={() => allow(key)}>Allow…</button>}</div>;
  return <><div className="dtoggle"><span><b>Let Beep use this computer</b><small>{sub}</small></span><Switch label="Let Beep use this computer" on={mac.enabled} id="usemac" disabled={busy} flip={() => setEnabled(!mac.enabled)} /></div>
    {mac.enabled && desktopControl && <>{permission("ScreenRecording", "Screen Recording", "To see the screen", p.screen_recording)}{permission("Accessibility", "Accessibility", "To click, type and read app controls", p.accessibility)}</>}
  </>;
}
