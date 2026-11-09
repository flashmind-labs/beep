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
