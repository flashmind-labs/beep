import type { ReactNode } from "react";
import { navEntries, subKey, type SettingsPage, type SettingsPath, type SettingsSub } from "./settings-nav.ts";

interface NavigationProps {
  pages: SettingsPage[]; path: SettingsPath; version: string;
  subs(page: SettingsPage): SettingsSub[]; pageName(page: SettingsPage): string; subName(sub: SettingsSub): string; count(page: SettingsPage): number;
  openPage(page: SettingsPage): void; openSub(sub: SettingsSub): void;
}
export function SettingsNavigation(p: NavigationProps) {
  return <><b>Settings</b><div className="dset-pages">{navEntries(p.pages, p.path, p.subs).map(entry => entry.sub
    ? <button className="cc-si sub" key={subKey(entry.sub)} data-sub={subKey(entry.sub)} aria-current={entry.current ? "true" : undefined} onClick={() => { if (entry.sub) p.openSub(entry.sub); }}><span className="tt">{p.subName(entry.sub)}</span></button>
    : <button className="cc-si" key={entry.page} data-goto={entry.page} aria-current={entry.current ? "true" : undefined} onClick={() => p.openPage(entry.page)}><span className="tt">{p.pageName(entry.page)}</span>{p.count(entry.page) > 0 && <span className="n">{p.count(entry.page)}</span>}</button>)}</div><small className="dset-ver">Beep desktop {p.version}</small></>;
}
export function SettingsBreadcrumb({ path, page, sub, openPage }: { path: SettingsPath; page: string; sub: string | null; openPage(page: SettingsPage): void }) {
  if (sub === null) return <b>{page}</b>;
  return <><button className="dset-back" data-goto={path.page} aria-label={`Back to ${page}`} onClick={() => openPage(path.page)}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M15 6l-6 6 6 6" /></svg></button><button className="dset-crumb" data-goto={path.page} onClick={() => openPage(path.page)}>{page}</button><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.6" strokeLinecap="round" strokeLinejoin="round"><path d="M9 6l6 6-6 6" /></svg><b>{sub}</b></>;
}

// Page contents retain their existing feature owner while the shell stays mounted.
export function SettingsShell({ navigation, breadcrumb, children }: { navigation: ReactNode; breadcrumb: ReactNode; children: ReactNode }) {
  return <><nav className="dset-nav" aria-label="Settings pages">{navigation}</nav><div className="dset-body"><div className="dset-top"><span className="dset-crumbs">{breadcrumb}</span><kbd>esc</kbd></div><div className="dset-page dform">{children}</div></div></>;
}

export function SettingsBarShell({ version, sections, hints }: { version: string; sections: ReactNode; hints: string }) {
  return <><div className="dhead"><span className="tt"><b>Settings</b><small>Beep desktop {version}</small></span></div><div className="dform dformcol">{sections}</div><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: hints }} /></>;
}

export function SettingsSection({ page, title }: { page: SettingsPage; title: string }) {
  return <section id={`set-${page}-sec`} data-sec={page}><div className="dsec flat top">{title}</div><div id={`set-${page}`} style={page === "general" || page === "mac" ? { display: "contents" } : {}} /></section>;
}

export function SettingsHost({ page, loading }: { page: SettingsPage; loading: boolean }) {
  return <section id={`set-${page}-sec`} data-sec={page}><div className="dset-host" id={`set-${page}-host`}>{loading && <div className="dskel-rows">{[78, 56, 68].map(width => <div key={width} className="dskel" style={{ width: `${width}%` }} />)}</div>}</div></section>;
}

export function AgentSettingsSection({ linked, sheet }: { linked: boolean; sheet: boolean }) {
  return <section id="set-agents-sec" data-sec="agents"><div className="dsec flat top">{linked ? "Agent accounts" : "Coding agents"}</div>
    {!sheet && <div id="set-agent-bar" style={{ display: "contents" }} />}
    {linked && <div id="set-agents" />}
    {sheet && <div id="set-agent-overview" style={{ display: "contents" }} />}
  </section>;
}


export function SettingsFailure({ error }: { error: string }) {
  return <div className="dwin"><h4>Settings</h4><p className="derr">{error}</p></div>;
}
