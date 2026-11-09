import { TABS, type MobileTab } from "./mobile-state.ts";

const PATHS: Record<MobileTab, string> = {
  home: "M3 11l9-7 9 7v9h-6v-6H9v6H3z",
  projects: "M3 7h7l2 2h9v11H3z",
  agents: "M4 5h16v11H9l-5 4z M8 9h8M8 12.5h5",
  terminals: "M4 5h16v14H4z M7 9l3 3-3 3M12 15h5",
  machines: "M3 4h18v6H3z M3 14h18v6H3z M7 7h.1M7 17h.1",
};
const BACK = "M15 5l-7 7 7 7";

const HIDE_KEYBOARD = "M2 5a2 2 0 0 1 2 -2h16a2 2 0 0 1 2 2v8a2 2 0 0 1 -2 2h-16a2 2 0 0 1 -2 -2l0 -8 M6 7l0 .01 M10 7l0 .01 M14 7l0 .01 M18 7l0 .01 M6 11l0 .01 M18 11l0 .01 M10 11l4 0 M10 21l2 -2l2 2";

function Icon({ path, size }: { path: string; size: number }) {
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.7} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={path} /></svg>;
}

export function MobileTabs({ active, needsReply, open }: { active: MobileTab | null; needsReply: boolean; open(tab: MobileTab): void }) {
  return <>{TABS.map(tab => <button type="button" key={tab.id} aria-current={tab.id === active ? "true" : undefined} className={tab.id === "home" && needsReply && active !== "home" ? "dot" : ""} onClick={() => open(tab.id)}><span className="ic"><Icon path={PATHS[tab.id]} size={21} /></span>{tab.label}</button>)}</>;
}

export function MobileBack({ hidden, back }: { hidden: boolean; back(): void }) {
  return <button type="button" className="mb-back" aria-label="Back" hidden={hidden} onClick={back}><Icon path={BACK} size={22} /></button>;
}

export function HideKeyboard({ dismiss }: { dismiss(): void }) {
  return <button type="button" className="mb-kbdhide" aria-label="Hide the keyboard" onPointerDown={event => event.preventDefault()} onClick={dismiss}><Icon path={HIDE_KEYBOARD} size={20} /></button>;
}
