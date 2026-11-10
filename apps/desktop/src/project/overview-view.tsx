import { Fragment, useLayoutEffect, useRef } from "react";
import { ago as itemAgo, plural } from "../ui/format.ts";
import { IC } from "../ui/icons.ts";
import { itemChips, type GitItemRow } from "../center/openitems-state.ts";
import { newWindowOn, openProjects, setProjectOnTop } from "../center/sidebar.ts";
import { agentLabel, foundSessions, type FoundRow, type OverviewCard, type OverviewGroup, type Project } from "./projects-state.ts";
import { ago } from "../addproject/addproject.ts";
import { OverviewCardView, overviewCardModel } from "./overview-card.tsx";
import { editGroup, groupMenu, removeGroup, renameGroup, startRename } from "./groups.ts";
import { state } from "../center/state.ts";
import { redrawProjects } from "./sections.ts";

type Group = OverviewGroup<Project, OverviewCard>;
export interface ElsewhereRepo { key: string; name: string; sub: string; url: string; last: number; importing: boolean; disabled: boolean; primary: boolean; items: GitItemRow[]; import(): void }
export function ElsewhereSection({ repos, open, review, reply }: { repos: ElsewhereRepo[]; open(url: string): void; review(item: GitItemRow): void; reply(item: GitItemRow): void }) {
  const act = (event: React.MouseEvent, run: () => void) => { event.stopPropagation(); run(); };
  return <><div className="cc-sec"><h4>Your open PRs and issues in repos without a project</h4></div><div className="cc-panel cc-open">{repos.map(repo => <Fragment key={repo.key}><div className="cc-row cc-line cc-gr" onClick={() => open(repo.url)}><span className="cc-pdot"/><button type="button" className="t" title={repo.name}><b>{repo.name}</b><span>{repo.sub}</span></button><span className="r"><span className="age">{itemAgo(repo.last)}</span><button type="button" className={`dbtn ${repo.primary ? "pri " : ""}small`} disabled={repo.disabled} onClick={event => act(event, repo.import)}>{repo.importing ? "Importing…" : "Import project"}</button></span></div>{repo.items.map(item => <div key={item.url} className="cc-row cc-line cc-gr it" onClick={() => open(item.url)}><span className="ic" dangerouslySetInnerHTML={{ __html: item.kind === "pull_request" ? IC.pr : IC.issue }}/><button type="button" className="t" title={item.url}><span className="num">#{item.number}</span><b>{item.title}</b>{item.labels?.map(label => <i key={label} className="pj-tag">{label}</i>)}</button><span className="r">{itemChips(item).map((chip, index) => <span className={chip.cls ? `chip ${chip.cls}` : "chip"} key={`${chip.text}:${index}`}>{chip.text}</span>)}<span className="age">{itemAgo(item.updated_at)}</span>{item.kind === "pull_request" && <button type="button" className="dbtn small" onClick={event => act(event, () => review(item))}>Review</button>}<button type="button" className="dbtn small" onClick={event => act(event, () => reply(item))}>Reply</button></span></div>)}</Fragment>)}</div></>;
}
export function FoundSection({ rows, importing, error, importOne, add }: { rows: FoundRow[]; importing: string | null; error: { name: string; message: string } | null; importOne(row: FoundRow): void; add(): void }) {
  return <><div className="cc-sec"><h4>Found on your machines</h4>{rows.length > 3 && <button type="button" className="dbtn quiet small" onClick={add}>All {rows.length}</button>}</div><div className="cc-panel ap-list">{rows.slice(0, 3).map(row => <div className="ap-row" key={row.name}><span className="n">{row.name}</span>{row.places.map(place => <span className="m" key={`${place.machine_name}:${place.folder}`}><span title={`${place.machine_name}: ${place.folder}`}><b>{place.machine_name}</b> <code>{place.folder}</code></span></span>)}<span className="m">{row.agents.map((agent, index) => <span className="ap-ag" key={index}>{agentLabel(agent.agent)} {agent.sessions}</span>)}{row.last_activity ? <span>{ago(row.last_activity)}</span> : null}</span>{error?.name === row.name && <span className="m bad">{error.message}</span>}<span className="acts"><button type="button" className="dbtn small" disabled={importing !== null} title={`Import ${row.name} with ${foundSessions(row)} sessions`} onClick={() => importOne(row)}>{importing === row.name ? "Importing…" : "Import"}</button></span></div>)}</div></>;
}
export function OverviewGroupView({ group, sync, editing }: { group: Group; sync: (name: string) => string | null; editing: { id: string; draft: string } | null }) {
  const g = group.group;
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (!editing || !input.current || document.activeElement === input.current) return;
    const field = input.current;
    const frame = requestAnimationFrame(() => { if (field.isConnected) { field.focus(); field.setSelectionRange(field.value.length, field.value.length); } });
    return () => cancelAnimationFrame(frame);
  }, [editing]);
  const finish = (save: boolean) => {
    if (!g || !editing || state.renaming !== editing) return;
    if (save) void renameGroup(g, input.current?.value ?? editing.draft);
    else { state.renaming = null; redrawProjects(); }
  };
  return <><h3 className={g ? "gp" : undefined} onContextMenu={g && !editing ? e => groupMenu(e.nativeEvent, g) : undefined}>
    {g ? <><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H10l2 2.5h7.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z" /></svg>{editing ? <><input ref={input} defaultValue={editing.draft} aria-label="Group name" onChange={e => { editing.draft = e.currentTarget.value; }} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); finish(true); } else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); } }} /><span className="gp-acts"><button type="button" className="dbtn pri" onClick={() => finish(true)}>Save</button><button type="button" className="dbtn quiet" onClick={() => finish(false)}>Cancel</button></span></> : <>{g.name}<small>{plural(group.cards.length, "repo", "repos")}</small><span className="gp-paths" title="Matched on every machine">{g.paths.map(p => p.startsWith("/") ? p : `~/${p}`).join(", ")}</span><span className="gp-acts"><button type="button" className="dbtn quiet" onClick={() => startRename(g)}>Rename</button><button type="button" className="dbtn quiet" onClick={() => editGroup(g)}>Folders…</button><button type="button" className="dbtn quiet warn" onClick={() => removeGroup(g)}>Remove</button></span></>}</> : <>{group.org ? `${group.org}/` : "Other folders"}<small>{plural(group.cards.length, "repo", "repos")}</small></>}
  </h3>{group.cards.length || !g ? <div className="pj-grid">{group.cards.map(c => <OverviewCardView key={c.name} model={overviewCardModel(c, sync(c.name))} open={openProjects} pin={setProjectOnTop} wire={newWindowOn} />)}</div> : <span className="gp-none">No projects in these folders yet.</span>}</>;
}
