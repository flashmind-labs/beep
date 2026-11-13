import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Input, SearchField } from "react-aria-components";
import { commandKey } from "../ui/platform.ts";
import { formatAgo, plural } from "../ui/format.ts";
import { Icon } from "../ui/icons.tsx";
import { itemChips, type GitItemRow } from "../center/openitems-state.ts";
import { projectWindowProps, openProjects, setProjectOnTop } from "../center/sidebar.ts";
import { agentLabel, foundSessions, parseFolded, type FoundRow, type Project } from "./projects-state.ts";
import { PROJECT_SORTS, parseProjectSort, searchOverview, type ProjectSort, type OverviewGroup } from "./overview-state.ts";
import { OverviewCardView, overviewCardModel } from "./overview-card.tsx";
import { editGroup, groupMenu, removeGroup, renameGroup, startRename } from "./groups.ts";
import { state } from "../center/state.ts";
import { redrawProjects } from "./sections.ts";
import { dialogOpen } from "../ui/parts.ts";
import { Button } from "../ui/kit.tsx";

type Group = OverviewGroup<Project>;
/// What the search field holds, kept while the user is on other pages.
let kept = "";

/// The groups folded on this page, kept between launches.
const FOLDED_KEY = "beep.overviewFolded";
let folded = new Set<string>();
try { folded = parseFolded(localStorage.getItem(FOLDED_KEY)); } catch { /* none kept */ }
const groupKey = (group: Group) => group.group ? `group:${group.group.id}` : `org:${group.org ?? ""}`;

export function ProjectsOverview({ groups: all, sync, editing, add, sort, changeSort }: { sort: ProjectSort; changeSort(sort: ProjectSort): void; groups: Group[]; sync(name: string): string | null; editing: { id: string; draft: string } | null; add(): void }) {
  const [query, setQuery] = useState(kept);
  const groups = useMemo(() => searchOverview(all, query), [all, query]);
  const field = useRef<HTMLInputElement>(null);
  useSearchKeys(field);
  const [count, setCount] = useState(() => typeof IntersectionObserver === "undefined" ? groups.length : 1), [height, setHeight] = useState(0);
  const first = useRef<HTMLElement>(null), end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const marker = end.current;
    if (!marker) return;
    if (typeof IntersectionObserver === "undefined") { setCount(groups.length); return; }
    let root: Element | null = marker.parentElement;
    while (root && !/(auto|scroll)/.test(getComputedStyle(root).overflowY)) root = root.parentElement;
    const observer = new IntersectionObserver(entries => {
      if (!height && first.current) {
        const measured = (marker.getBoundingClientRect().top - first.current.getBoundingClientRect().top) / Math.min(count, groups.length);
        if (measured > 0) setHeight(measured);
      }
      if (entries.some(entry => entry.isIntersecting)) setCount(value => Math.min(groups.length, value + 1));
    }, { root, rootMargin: "800px 0px" });
    observer.observe(marker);
    return () => observer.disconnect();
  }, [count, groups.length, height]);
  const search = (text: string) => { kept = text; setQuery(text); };
  const [shut, setShut] = useState(folded);
  const fold = (key: string) => {
    const next = new Set(shut);
    if (!next.delete(key)) next.add(key);
    folded = next;
    setShut(next);
    try { localStorage.setItem(FOLDED_KEY, JSON.stringify([...next])); } catch { /* kept for this launch only */ }
  };
  // A search shows every match, folded or not.
  const searching = !!query.trim();

  return <><div className="pj-toolbar"><SearchField className="pj-find" aria-label="Search projects" value={query} onChange={search}><Input ref={field} placeholder="Search projects" spellCheck={false} autoComplete="off"/></SearchField><select className="dbtn pj-sort" aria-label="Sort projects" value={sort} onChange={event => changeSort(parseProjectSort(event.currentTarget.value))}>{PROJECT_SORTS.map(option => <option key={option.id} value={option.id}>{option.label}</option>)}</select></div>{!groups.length && query.trim() && <div className="cc-panel cc-empty pj-none"><span>No projects match.</span></div>}{groups.slice(0, count).map((group, index) => <section className="pj-org" style={{ flex: "none" }} ref={index === 0 ? first : null} key={groupKey(group)}><OverviewGroupView group={group} editing={group.group && editing?.id === group.group.id ? editing : null} sync={sync} folded={!searching && shut.has(groupKey(group))} fold={() => fold(groupKey(group))}/></section>)}{count < groups.length && <div ref={end} style={{ flex: "none", gridColumn: "1 / -1", height: height ? `${(groups.length - count) * height}px` : undefined }}/>}<div className="cc-panel cc-row pj-add" style={{ flex: "none" }}><span className="t"><b>Add a project</b><span>A folder or a repository to clone.</span></span><Button  onClick={add}>Add</Button></div></>;
}
/// `/` or Cmd/Ctrl+F focuses the search while the overview shows and no
/// field, dialog or other handler has the key.
function useSearchKeys(field: React.RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    const keys = (event: KeyboardEvent) => {
      const input = field.current;
      if (!input || event.defaultPrevented || event.isComposing || event.altKey || !input.checkVisibility() || dialogOpen()) return;

      const target = event.target instanceof HTMLElement ? event.target : null;
      const typing = !!target && (target.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName));
      const find = commandKey(event) && !event.shiftKey && event.key.toLowerCase() === "f";
      const slash = event.key === "/" && !event.metaKey && !event.ctrlKey && !typing;
      if (!find && !slash) return;

      event.preventDefault();
      input.focus();
      input.select();
    };
    document.addEventListener("keydown", keys);
    return () => document.removeEventListener("keydown", keys);
  }, [field]);
}

interface ElsewhereRepo { key: string; name: string; sub: string; url: string; last: number; importing: boolean; disabled: boolean; primary: boolean; items: GitItemRow[]; import(): void }
export function ElsewhereSection({ repos, open, review, reply }: { repos: ElsewhereRepo[]; open(url: string): void; review(item: GitItemRow): void; reply(item: GitItemRow): void }) {
  const act = (event: React.MouseEvent, run: () => void) => { event.stopPropagation(); run(); };
  return <><div className="cc-sec"><h4>Your open PRs and issues in repos without a project</h4></div><div className="cc-panel cc-open">{repos.map(repo => <Fragment key={repo.key}><div className="cc-row cc-line cc-gr" onClick={() => open(repo.url)}><span className="cc-pdot"/><button type="button" className="t" title={repo.name}><b>{repo.name}</b><span>{repo.sub}</span></button><span className="r"><span className="age">{formatAgo(repo.last, "short")}</span><Button look={repo.primary ? "primary" : "secondary"} small disabled={repo.disabled} onClick={event => act(event, repo.import)}>{repo.importing ? "Importing…" : "Import project"}</Button></span></div>{repo.items.map(item => <div key={item.url} className="cc-row cc-line cc-gr it" onClick={() => open(item.url)}><span className="ic"><Icon name={item.kind === "pull_request" ? "pr" : "issue"}/></span><button type="button" className="t" title={item.url}><span className="num">#{item.number}</span><b>{item.title}</b>{item.labels?.map(label => <i key={label} className="pj-tag">{label}</i>)}</button><span className="r">{itemChips(item).map((chip, index) => <span className={chip.cls ? `chip ${chip.cls}` : "chip"} key={`${chip.text}:${index}`}>{chip.text}</span>)}<span className="age">{formatAgo(item.updated_at, "short")}</span>{item.kind === "pull_request" && <Button small onClick={event => act(event, () => review(item))}>Review</Button>}<Button small onClick={event => act(event, () => reply(item))}>Reply</Button></span></div>)}</Fragment>)}</div></>;
}
export function FoundSection({ rows, importing, error, importOne, add }: { rows: FoundRow[]; importing: string | null; error: { name: string; message: string } | null; importOne(row: FoundRow): void; add(): void }) {
  return <><div className="cc-sec"><h4>Found on your machines</h4>{rows.length > 3 && <Button look="quiet" small onClick={add}>All {rows.length}</Button>}</div><div className="cc-panel ap-list">{rows.slice(0, 3).map(row => <div className="ap-row" key={row.name}><span className="n">{row.name}</span>{row.places.map(place => <span className="m" key={`${place.machine_name}:${place.folder}`}><span title={`${place.machine_name}: ${place.folder}`}><b>{place.machine_name}</b> <code>{place.folder}</code></span></span>)}<span className="m">{row.agents.map((agent, index) => <span className="ap-ag" key={index}>{agentLabel(agent.agent)} {agent.sessions}</span>)}{row.last_activity ? <span>{formatAgo(row.last_activity, "long")}</span> : null}</span>{error?.name === row.name && <span className="m bad">{error.message}</span>}<span className="acts"><Button small disabled={importing !== null} title={`Import ${row.name} with ${foundSessions(row)} sessions`} onClick={() => importOne(row)}>{importing === row.name ? "Importing…" : "Import"}</Button></span></div>)}</div></>;
}
function OverviewGroupView({ group, sync, editing, folded, fold }: { group: Group; sync: (name: string) => string | null; editing: { id: string; draft: string } | null; folded: boolean; fold(): void }) {
  const g = group.group;
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    if (!editing || !input.current || document.activeElement === input.current) return;
    const field = input.current;
    const frame = requestAnimationFrame(() => { if (field.isConnected && document.activeElement !== field) { field.focus(); field.setSelectionRange(field.value.length, field.value.length); } });
    return () => cancelAnimationFrame(frame);
  }, [editing]);
  const finish = (save: boolean) => {
    if (!g || !editing || state.renaming !== editing) return;
    if (save) void renameGroup(g, input.current?.value ?? editing.draft);
    else { state.renaming = null; redrawProjects(); }
  };
  const icon = g ? <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H10l2 2.5h7.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z" /></svg> : null;
  const label = g ? g.name : group.org ? `${group.org}/` : "Other folders";
  const name = <button type="button" className="pj-fold" aria-expanded={!folded} onClick={fold}><span className={`twist${folded ? "" : " open"}`}><Icon name="twist" /></span>{icon}<span className="nm" title={label}>{label}</span><small>{plural(group.cards.length, "repo", "repos")}</small></button>;
  return <><h3 className={g ? "gp" : undefined} onContextMenu={g && !editing ? e => groupMenu(e.nativeEvent, g) : undefined}>
    {g && editing ? <>{icon}<input ref={input} defaultValue={editing.draft} aria-label="Group name" onChange={e => { editing.draft = e.currentTarget.value; }} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); finish(true); } else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); } }} /><span className="gp-acts"><Button look="primary" onClick={() => finish(true)}>Save</Button><Button look="quiet" onClick={() => finish(false)}>Cancel</Button></span></>
      : g ? <>{name}<span className="gp-paths" title="Matched on every machine">{g.paths.map(p => p.startsWith("/") ? p : `~/${p}`).join(", ")}</span><span className="gp-acts"><Button look="quiet" onClick={() => startRename(g)}>Rename</Button><Button look="quiet" onClick={() => editGroup(g)}>Folders…</Button><Button look="quiet" danger onClick={() => removeGroup(g)}>Remove</Button></span></>
      : name}
  </h3>{folded ? null : group.cards.length || !g ? <div className="pj-grid">{group.cards.map(c => <OverviewCardView key={c.name} model={overviewCardModel(c, sync(c.name))} open={openProjects} pin={setProjectOnTop} windowProps={projectWindowProps(c.name)} />)}</div> : <span className="gp-none">No projects in these folders yet.</span>}</>;
}
