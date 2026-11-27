import { Icon, type IconName } from "../../ui/icons.tsx";
import { TABS, type MobileTab } from "./mobile-state.ts";

const TAB_ICONS: Record<MobileTab, IconName> = { home: "tabHome", projects: "tabProjects", agents: "tabAgents", terminals: "tabTerminals", machines: "tabMachines" };

export function MobileTabs({ active, needsReply, open }: { active: MobileTab | null; needsReply: boolean; open(tab: MobileTab): void }) {
  return <>{TABS.map(tab => <button type="button" key={tab.id} aria-current={tab.id === active ? "true" : undefined} className={tab.id === "home" && needsReply && active !== "home" ? "dot" : ""} onClick={() => open(tab.id)}><span className="ic"><Icon name={TAB_ICONS[tab.id]} /></span>{tab.label}</button>)}</>;
}

export function MobileBack({ hidden, back }: { hidden: boolean; back(): void }) {
  return <button type="button" className="mb-back" aria-label="Back" hidden={hidden} onClick={back}><Icon name="back" /></button>;
}

/// Settings from the top bar: the phone has no sidebar to hold it.
export function MobileSettings({ hidden, open }: { hidden: boolean; open(): void }) {
  return <button type="button" className="mb-set" aria-label="Settings" hidden={hidden} onClick={open}><Icon name="gear" /></button>;
}

export function HideKeyboard({ dismiss }: { dismiss(): void }) {
  return <button type="button" className="mb-kbdhide" aria-label="Hide the keyboard" onPointerDown={event => event.preventDefault()} onClick={dismiss}><Icon name="hideKeyboard" /></button>;
}
