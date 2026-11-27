import { lazy, Suspense, useState } from "react";
import { SKINS, frontLayout, pluginSkinId, setFrontLayout, setSkin, setSystemSkin, skinPlugins, skinPreference, type FrontLayout, type FrontPage } from "../ui/skin.ts";
import { useStore } from "zustand";
import { conversationAppearance, setConversationAppearance } from "../ui/conversation-appearance.ts";
import { WINDOWS } from "../ui/platform.ts";
import type { MacControl } from "../machines/machine-state.ts";
import { Button, Switch } from "../ui/kit.tsx";
import { SettingRow } from "./overview-view.tsx";
import { useBuildCapabilities } from "../ui/tauri.ts";

const LAYOUTS: readonly { id: FrontLayout; label: string }[] = [{ id: "board", label: "Board" }, { id: "list", label: "List" }];
const FRONT_PAGES: readonly { page: FrontPage; name: string; note: string }[] = [
  { page: "project", name: "Projects", note: "A project's front page" },
];
// React Aria sets up global listeners on load, so it loads with the view that
// shows it rather than with Settings.
const LayoutSegmented = lazy(() => import("../ui/aria-kit.tsx").then(({ Segmented }) => ({ default: Segmented<FrontLayout> })));

function LayoutPick({ page, name, note }: { page: FrontPage; name: string; note: string }) {
  const [layout, setLayout] = useState(() => frontLayout(page));
  return <SettingRow title={name} note={note}>
    <Suspense fallback={null}><LayoutSegmented label={`${name} layout`} options={LAYOUTS} value={layout} onChange={next => { setFrontLayout(page, next); setLayout(next); }}/></Suspense></SettingRow>;
}

export function AppearanceView() {
  const pref = useStore(skinPreference);
  const appearance = useStore(conversationAppearance);
  const plugins = useStore(skinPlugins, state => state.skins);
  const groups = [
    { kind: "dark" as const, label: "Dark", skins: SKINS.filter(skin => !skin.light) },
    { kind: "light" as const, label: "Light", skins: SKINS.filter(skin => skin.light) },
    { kind: "plugin" as const, label: "Plugins", skins: plugins.map(skin => ({ ...skin, id: pluginSkinId(skin.id) })) },
  ].filter(group => group.skins.length > 0);
  const darkName = SKINS.find(skin => skin.id === pref.dark)?.name ?? "Linear";
  const lightName = SKINS.find(skin => skin.id === pref.light)?.name ?? "Light";
  const themeNote = pref.system
    ? `Switches between ${darkName} and ${lightName} with the system appearance`
    : "Switch between dark and light skins with the system appearance";
  const pressed = (kind: "dark" | "light" | "plugin", id: string) =>
    pref.system ? (kind === "dark" ? id === pref.dark : kind === "light" ? id === pref.light : false) : id === pref.active;
  return <>
    <SettingRow title="System theme" note={themeNote}><Switch id="systemTheme" label="System theme" on={pref.system} flip={() => setSystemSkin(!pref.system)} /></SettingRow>
    {groups.map(group => <div className="dskins" key={group.label}><small>{group.label}</small><div>{group.skins.map(skin => <button className="dskin" key={skin.id} data-skin-pick={skin.id} aria-pressed={pressed(group.kind, skin.id)} onClick={() => setSkin(skin.id)}><i style={{ background: skin.swatch }} />{skin.name}</button>)}</div></div>)}
    {FRONT_PAGES.map(front => <LayoutPick key={front.page} {...front}/>)}
    <SettingRow title="Conversation scrollbar" note="Show the scrollbar beside agent conversations"><Switch label="Conversation scrollbar" on={appearance.scrollbar} flip={() => setConversationAppearance("scrollbar", !appearance.scrollbar)} /></SettingRow>
    <SettingRow title="Prompt marks" note="Mark your messages in the conversation scrollbar"><Switch label="Prompt marks" on={appearance.promptMarks} flip={() => setConversationAppearance("promptMarks", !appearance.promptMarks)} /></SettingRow>
  </>;
}
export function ComputerView({ mac, busy, setEnabled, allow }: { mac: MacControl; busy: boolean; setEnabled(enabled: boolean): void; allow(permission: "ScreenRecording" | "Accessibility"): void }) {
  const capabilities = useBuildCapabilities();
  if (!capabilities.local_host) return null;
  const p = mac.permissions, desktopControl = !WINDOWS && capabilities.desktop_control;
  const sub = !mac.enabled ? desktopControl ? "Beep can see the screen, click, type and run commands here when you ask" : "Beep can run commands and coding agents here when you ask"
    : mac.status === "failed" ? `Couldn't connect: ${mac.error || "unknown error"}` : mac.status === "connected" ? !desktopControl || p.screen_recording && p.accessibility ? "Connected" : "Connected. Allow both below" : "Connecting…";
  const permission = (key: "ScreenRecording" | "Accessibility", name: string, why: string, ok: boolean) => <SettingRow key={key} title={name} note={why}>{ok ? <span className="dtag ok">Allowed</span> : <Button data-allow={key} onClick={() => allow(key)}>Allow…</Button>}</SettingRow>;
  return <><SettingRow title="Let Beep use this computer" note={sub}><Switch label="Let Beep use this computer" on={mac.enabled} id="usemac" disabled={busy} flip={() => setEnabled(!mac.enabled)} /></SettingRow>
    {mac.enabled && desktopControl && <>{permission("ScreenRecording", "Screen Recording", "To see the screen", p.screen_recording)}{permission("Accessibility", "Accessibility", "To click, type and read app controls", p.accessibility)}</>}
  </>;
}
