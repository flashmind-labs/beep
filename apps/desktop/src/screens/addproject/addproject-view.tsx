import { Fragment, useLayoutEffect, useState, type RefObject } from "react";
import { useStore } from "zustand";
import { SyncthingChoice } from "../project/syncthing-choice.tsx";
import type { SyncthingImportChoice } from "../project/syncthing.ts";
import { agentLabel, foundSessions, lookingText, type FoundRow } from "../../stores/project/projects-state.ts";
import { emptyText, lookingOn, tickable, toImport, type ImportState } from "../../stores/addproject/addproject-state.ts";
import type { HostRepos, AddProjectOptions } from "./addproject.tsx";
import { formatAgo } from "../../ui/format.ts";
import { Button, Checkbox, NO_AUTO, SheetHead, Spinner } from "../../ui/kit.tsx";

interface Props {
  found: FoundRow[] | null; shown: FoundRow[]; hosts: HostRepos[] | null; machines: AddProjectOptions["machines"]; query: string; looking: string[]; unread: string[]; error: string | null; importing: string | null; batch: boolean; picked: ReadonlySet<string>; status: ReadonlyMap<string, ImportState>; failed: ReadonlyMap<string, string>; sync: SyncthingImportChoice;
  searchField: RefObject<HTMLInputElement | null>; rowBox: RefObject<HTMLDivElement | null>;
  search(value: string): void; toggle(name: string): void; toggleAll(): void; importPicked(): void; importOne(row: FoundRow): void; choose(): void; clone(repo?: string): void; create(): void;
}
export function AddProjectView(p: Props) {
  const [browseOpen, setBrowseOpen] = useState(false), syncthing = useStore(p.sync.state).available;
  const still = lookingOn(p.looking), any = !!p.found?.length;
  const remote = !!p.hosts?.some(h => h.repos.length), names = p.shown.map(row => row.name);
  const open = names.filter(name => tickable(p.status.get(name))), ready = toImport(names, p.picked, p.status).length;
  const checked = open.length > 0 && open.every(name => p.picked.has(name));
  const have = new Set((p.found ?? []).map(row => row.name.toLowerCase()));
  const words = p.query.toLowerCase().split(/\s+/).filter(Boolean);
  const sections = (p.hosts ?? []).flatMap(host => {
    const repos = host.repos.filter(repo => !have.has(repo.name.toLowerCase()) && words.every(word => repo.name.toLowerCase().includes(word))).slice(0, 30);
    return host.error || repos.length ? [{ host, repos }] : [];
  });
  const input = p.searchField;
  useLayoutEffect(() => { if ((any || remote) && !document.activeElement?.closest(".ap-sheet")) input.current?.focus(); }, [any, remote, input]);
  return <><SheetHead title="Add a project" /><div className="ap-ways"><Way name="Choose folder" hint="Already on a machine" run={p.choose} /><Way name="Clone repository" hint="From a URL or a host" run={() => p.clone()} /><Way name="New project" hint="Start from nothing" run={p.create} /></div>
    <div className="am-label">On your machines</div><input ref={input} className="ap-q" type="search" placeholder="Filter by repo, folder, machine or agent" {...NO_AUTO} aria-label="Filter found folders" hidden={!any && !remote} value={p.query} onChange={e => p.search(e.currentTarget.value)} />
    <div className="ap-bar" hidden={!any}><label><Checkbox checked={checked} mixed={!checked && open.some(name => p.picked.has(name))} disabled={p.batch || !open.length} change={p.toggleAll} />Select all shown</label><span>{p.query.trim() ? `${p.shown.length} of ${p.found?.length ?? 0} shown` : `${p.found?.length ?? 0} found`}</span><Button look="primary" small disabled={!!p.importing || p.batch || !ready} onClick={p.importPicked}>{p.batch ? "Importing…" : `Import ${ready}`}<kbd>⌘↵</kbd></Button></div>
    <div className="am-wait ap-look" hidden={!still || !any}><Spinner />{still}</div><div className="ap-results"><div className="ap-box" ref={p.rowBox}>{p.found === null ? <div className="am-wait"><Spinner />{lookingText(p.machines)}</div> : !any ? (() => { const empty = emptyText(p.looking, p.unread, p.error); return empty ? <div className="am-sub">{empty}</div> : <div className="am-wait"><Spinner />{still}</div>; })() : !p.shown.length ? <div className="am-sub">Nothing found matches “{p.query.trim()}”.</div> : <div className="ap-list">{p.shown.map(row => <FoundRowView key={row.name} row={row} state={p.status.get(row.name)} error={p.failed.get(row.name) ?? null} busy={p.importing ?? (p.batch ? "" : null)} selected={p.picked.has(row.name)} toggle={() => p.toggle(row.name)} onImport={() => p.importOne(row)} />)}</div>}{any && !!p.unread.length && <div className="am-sub">{p.unread.join(" ")}</div>}</div>
    <div className="ap-remote">{!!sections.length && <details open={!!p.query.trim() || browseOpen} onToggle={e => setBrowseOpen(e.currentTarget.open)}><summary>Browse repositories on connected hosts</summary>{sections.map(({ host, repos }) => <Fragment key={host.host}><div className="am-label">On {host.github ? "GitHub" : host.host}</div>{host.error ? <div className="am-sub">{host.error}</div> : <div className="ap-list">{repos.map(repo => <div className="ap-rm" key={repo.url}><span className="n">{repo.name}</span><span className="m">{formatAgo(repo.updated_at, "long")}</span><Button small onClick={() => p.clone(repo.url)}>Clone</Button></div>)}</div>}</Fragment>)}</details>}</div></div>
    {syncthing && <details className="ap-sync"><summary>Sync options</summary><SyncthingChoice choice={p.sync} /></details>}</>;
}
/// One way to add a project: its name, and on the desktop a line saying when to pick it.
function Way({ name, hint, run }: { name: string; hint: string; run(): void }) {
  return <Button small aria-label={name} onClick={run}><b>{name}</b><span>{hint}</span></Button>;
}
function FoundRowView({ row, state, error, busy, selected, toggle, onImport }: { row: FoundRow; state: ImportState | undefined; error: string | null; busy: string | null; selected: boolean; toggle(): void; onImport(): void }) {
  const why = state?.state === "failed" ? `Failed: ${state.why}` : error;
  return <div className="ap-row ap-pick" tabIndex={0} data-name={row.name} aria-selected={selected} onClick={toggle}><Checkbox checked={selected} disabled={!tickable(state)} tabIndex={-1} label={`Select ${row.name}`} contained change={toggle} /><span className="n">{row.name}</span>{row.places.map(p => <span className="m" key={`${p.machine_name}:${p.folder}`}><span title={`${p.machine_name}: ${p.folder}`}><b>{p.machine_name}</b> <code>{p.folder}</code></span></span>)}<span className="m">{row.agents.map((a, i) => <span className="ap-ag" key={i}>{agentLabel(a.agent)} {a.sessions}</span>)}{row.last_activity ? <span>{formatAgo(row.last_activity, "long")}</span> : null}</span>{why && <span className="m bad">{why}</span>}<span className="acts" onClick={e => e.stopPropagation()}>{state?.state === "importing" ? <span className="ap-st"><Spinner />Importing…</span> : state?.state === "done" ? <span className="ap-st ok">Imported</span> : <Button small disabled={busy !== null} title={`Import ${row.name} with ${foundSessions(row)} sessions`} onClick={onImport}>{busy === row.name ? "Importing…" : state?.state === "failed" ? "Retry" : "Import"}</Button>}</span></div>;
}
