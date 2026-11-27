import { HintView, type HintViewModel } from "../../parts/bar/hints-view.tsx";
import { Icon, type IconName } from "../../ui/icons.tsx";
import { Button, NO_AUTO } from "../../ui/kit.tsx";
import { Fragment, useLayoutEffect, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
import { onPhone } from "../../ui/platform.ts";
import { pointedSetting } from "./overview-view.tsx";
import { navEntries, pageGroup, searchEntries, subKey, type NavEntry, type SettingsPage, type SettingsPath, type SettingsSub } from "../../stores/settings/settings-nav.ts";

const PAGE_ICONS: Record<SettingsPage, IconName> = {
  general: "gear",
  appearance: "sliders",
  mac: "laptop",
  machines: "tabMachines",
  agents: "user",
  assistant: "chat",
  creds: "key",
  instr: "book",
  skills: "skill",
  plugins: "plugin",
};

interface NavigationProps {
  pages: SettingsPage[]; path: SettingsPath; version: string;
  subs(page: SettingsPage): SettingsSub[]; searchSubs(page: SettingsPage): SettingsSub[]; pageName(page: SettingsPage): string; subName(sub: SettingsSub): string; count(page: SettingsPage): number;
  openPage(page: SettingsPage): void; openSub(sub: SettingsSub): void;
}
export function SettingsNavigation(p: NavigationProps) {
  const [query, setQuery] = useState("");
  const searching = query.trim() !== "";
  const entries = searching ? searchEntries(p.pages, p.path, query, p.searchSubs, p.subName) : navEntries(p.pages, p.path, p.subs);
  const open = (entry: NavEntry) => {
    setQuery("");
    if (entry.sub) p.openSub(entry.sub);
    else p.openPage(entry.page);
    if (entry.setting) pointedSetting.setState({ title: entry.setting });
  };
  let lastGroup: string | null = null;

  return <><b>Settings</b><div className="dset-find-wrap"><Icon name="find" size={13} /><input className="dset-find" type="search" placeholder="Search settings" aria-label="Search settings" {...NO_AUTO} autoFocus={!onPhone()} value={query}
    onChange={e => setQuery(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter" && entries[0]) { e.preventDefault(); open(entries[0]); } }} />{!searching && <kbd className="dset-find-key">/</kbd>}</div>
    <div className="dset-pages">{entries.map(entry => {
      if (entry.setting) {
        return <button className="cc-si" key={`setting:${entry.page}:${entry.setting}`} onClick={() => open(entry)}><Icon name={PAGE_ICONS[entry.page]} size={14} /><span className="tt">{entry.setting}</span><span className="n">{p.pageName(entry.page)}</span></button>;
      }
      if (entry.sub) {
        return <button className={searching ? "cc-si" : "cc-si sub"} key={subKey(entry.sub)} data-sub={subKey(entry.sub)} aria-current={entry.current ? "true" : undefined} onClick={() => open(entry)}><span className="tt">{p.subName(entry.sub)}</span>{searching && <span className="n">{p.pageName(entry.page)}</span>}</button>;
      }
      const group = searching ? null : pageGroup(entry.page);
      const showGroup = group !== null && group !== lastGroup;
      if (group !== null) lastGroup = group;
      return <Fragment key={entry.page}>{showGroup && <div className="dset-grp">{group}</div>}<button className="cc-si" data-goto={entry.page} aria-current={entry.current ? "true" : undefined} onClick={() => open(entry)}><Icon name={PAGE_ICONS[entry.page]} size={14} /><span className="tt">{p.pageName(entry.page)}</span>{p.count(entry.page) > 0 && <span className="n">{p.count(entry.page)}</span>}</button></Fragment>;
    })}
      {searching && !entries.length && <small className="dset-none">No settings match.</small>}</div><small className="dset-ver">Beep desktop {p.version}</small></>;
}
export function SettingsBreadcrumb({ path, page, sub, openPage }: { path: SettingsPath; page: string; sub: string | null; openPage(page: SettingsPage): void }) {
  if (sub === null) return <b>{page}</b>;
  return <><button className="dset-back" data-goto={path.page} aria-label={`Back to ${page}`} onClick={() => openPage(path.page)}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M15 6l-6 6 6 6" /></svg></button><button className="dset-crumb" data-goto={path.page} onClick={() => openPage(path.page)}>{page}</button><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M9 6l6 6-6 6" /></svg><b>{sub}</b></>;
}

// The shell renders each page's content as its children; the sheet and bar share one owner each.
export function SettingsShell({ navigation, breadcrumb, close, children }: { navigation: ReactNode; breadcrumb: ReactNode; close(): void; children: ReactNode }) {
  return <><nav className="dset-nav" aria-label="Settings pages">{navigation}</nav><div className="dset-body"><div className="dset-top"><span className="dset-crumbs">{breadcrumb}</span><kbd>esc</kbd><Button look="quiet" className="dset-close" onClick={close}>Close</Button></div><div className="dset-page dform">{children}</div></div></>;
}

export function SettingsBarShell({ version, sections, hints }: { version: string; sections: ReactNode; hints: HintViewModel }) {
  return <><div className="dhead"><span className="tt"><b>Settings</b><small>Beep desktop {version}</small></span></div><div className="dform dformcol">{sections}</div><HintView value={hints} /></>;
}

export function SettingsSection({ page, title, children }: { page: SettingsPage; title: string; children: ReactNode }) {
  return <section id={`set-${page}-sec`} data-sec={page}><div className="dsec flat top">{title}</div><div id={`set-${page}`} style={page === "general" || page === "mac" ? { display: "contents" } : {}}>{children}</div></section>;
}

/// A page drawn by its own code: Machines and the embedded window views draw into `hostRef`.
export function SettingsHost({ page, hostRef }: { page: SettingsPage; hostRef: Ref<HTMLDivElement> }) {
  return <section id={`set-${page}-sec`} data-sec={page}><div className="dset-host" id={`set-${page}-host`} ref={hostRef} /></section>;
}

export type AgentsTab = "accounts" | "config";

export function AgentSettingsSection({ bar, accounts, overview, tab = "accounts", onTab }: {
  bar: ReactNode; accounts: ReactNode; overview: ReactNode; tab?: AgentsTab; onTab?(tab: AgentsTab): void;
}) {
  const tabbed = bar === null && overview !== null && onTab !== undefined;
  const flip = (next: AgentsTab) => (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    onTab?.(next);
    (event.currentTarget.parentElement?.querySelector<HTMLButtonElement>(`[data-tab="${next}"]`))?.focus();
  };

  return <section id="set-agents-sec" data-sec="agents"><div className="dsec flat top">Agents</div>
    {bar !== null && <div id="set-agent-bar" style={{ display: "contents" }}>{bar}</div>}
    {tabbed && <div className="dset-tabs" role="tablist" aria-label="Agents sections">
      <button type="button" role="tab" data-tab="accounts" aria-selected={tab === "accounts"} onClick={() => onTab("accounts")} onKeyDown={flip("config")}>Accounts</button>
      <button type="button" role="tab" data-tab="config" aria-selected={tab === "config"} onClick={() => onTab("config")} onKeyDown={flip("accounts")}>Configuration</button>
    </div>}
    {(!tabbed || tab === "accounts") && <div id="set-agents">{accounts}</div>}
    {overview !== null && (!tabbed || tab === "config") && <div id="set-agent-overview" style={{ display: "contents" }}>{overview}</div>}
  </section>;
}

/// Runs `entered` when its content mounts and `left` when it unmounts, so a
/// section can drop pending work that belongs to the copy that left.
export function Mounted({ entered, left, children }: { entered(): void; left(): void; children: ReactNode }) {
  useLayoutEffect(() => { entered(); return left; }, [entered, left]);
  return children;
}


export function SettingsFailure({ error }: { error: string }) {
  return <div className="dwin"><h4>Settings</h4><p className="derr">{error}</p></div>;
}


export function SettingsLoading() {
  return <><nav className="dset-nav"><b>Settings</b></nav><div className="dset-body"><div className="dset-top"><span className="tt"><b>Settings</b></span><kbd>esc</kbd></div>{[80, 60, 72, 50].map(width => <div className="cc-skel" key={width} style={{ width: `${width}%` }} />)}</div></>;
}
