import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { HazeArt } from "../ui/painted-art.tsx";
import { Input } from "react-aria-components/Input";
import { SearchField } from "react-aria-components/SearchField";
import { commandKey } from "../ui/platform.ts";
import { plural } from "../ui/format.ts";
import { Dropdown } from "../ui/dropdown.tsx";
import { Icon } from "../ui/icons.tsx";
import { projectWindowProps, setProjectOnTop } from "./projectops.ts";
import { openProjects } from "./addflow.ts";
import { parseFolded, type Project } from "./projects-state.ts";
import { PROJECT_SORTS, searchOverview, type ProjectSort, type OverviewGroup } from "./overview-state.ts";
import { OverviewCardView, overviewCardModel } from "./overview-card.tsx";
import { editGroup, groupMenu, removeGroup, renameGroup, startRename } from "./groups.tsx";
import { state } from "../center/state.ts";
import { redrawProjects } from "./sections.ts";
import { dialogOpen } from "../ui/parts.tsx";
import { Button, NO_AUTO, useLazyCount } from "../ui/kit.tsx";

type Group = OverviewGroup<Project>;
/// What the search field holds, kept while the user is on other pages.
let kept = "";

/// The groups folded on this page, kept between launches.
const FOLDED_KEY = "beep.overviewFolded";
let folded = new Set<string>();
try { folded = parseFolded(localStorage.getItem(FOLDED_KEY)); } catch { /* none kept */ }
const groupKey = (group: Group) => group.group ? `group:${group.group.id}` : `org:${group.org ?? ""}`;

/// `active` is the projects with open sessions and what each session does, over every project's card.
export function ProjectsOverview({ groups: all, active, sync, editing, add, sort, changeSort }: { sort: ProjectSort; changeSort(sort: ProjectSort): void; groups: Group[]; active: ReactNode; sync(name: string): string | null; editing: { id: string; draft: string } | null; add(): void }) {
  const [query, setQuery] = useState(kept);
  const groups = useMemo(() => searchOverview(all, query), [all, query]);
  const field = useRef<HTMLInputElement>(null);
  useSearchKeys(field);
  const { count, height, end, first } = useLazyCount<HTMLElement>(groups.length, () => typeof IntersectionObserver === "undefined" ? groups.length : 1, 1);
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

  return <><HazeArt /><div className="pj-toolbar"><SearchField className="pj-find" aria-label="Search projects" value={query} onChange={search}><Input ref={field} placeholder="Search projects" {...NO_AUTO}/></SearchField><Dropdown label="Sort projects" options={PROJECT_SORTS} value={sort} onChange={changeSort} icon={<Icon name="sort"/>}/></div>{!searching && active}{!groups.length && query.trim() && <div className="cc-panel cc-empty pj-none"><span>No projects match.</span></div>}{groups.slice(0, count).map((group, index) => <section className="pj-org" style={{ flex: "none" }} ref={index === 0 ? first : null} key={groupKey(group)}><OverviewGroupView group={group} editing={group.group && editing?.id === group.group.id ? editing : null} sync={sync} folded={!searching && shut.has(groupKey(group))} fold={() => fold(groupKey(group))}/></section>)}{count < groups.length && <div ref={end} style={{ flex: "none", gridColumn: "1 / -1", height: height ? `${(groups.length - count) * height}px` : undefined }}/>}<div className="cc-panel cc-row pj-add" style={{ flex: "none" }}><span className="t"><b>Add a project</b><span>A folder or a repository to clone.</span></span><Button onClick={add}>Add</Button></div></>;
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
    {g && editing ? <>{icon}<input ref={input} defaultValue={editing.draft} {...NO_AUTO} aria-label="Group name" onChange={e => { editing.draft = e.currentTarget.value; }} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); finish(true); } else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); finish(false); } }} /><span className="gp-acts"><Button look="primary" onClick={() => finish(true)}>Save</Button><Button look="quiet" onClick={() => finish(false)}>Cancel</Button></span></>
      : g ? <>{name}<span className="gp-paths" title="Matched on every machine">{g.paths.map(p => p.startsWith("/") ? p : `~/${p}`).join(", ")}</span><span className="gp-acts"><Button look="quiet" onClick={() => startRename(g)}>Rename</Button><Button look="quiet" onClick={() => editGroup(g)}>Folders…</Button><Button look="quiet" danger onClick={() => removeGroup(g)}>Remove</Button></span></>
      : name}
  </h3>{folded ? null : group.cards.length || !g ? <div className="pj-grid">{group.cards.map(c => <OverviewCardView key={c.name} model={overviewCardModel(c, sync(c.name))} open={openProjects} pin={setProjectOnTop} windowProps={projectWindowProps(c.name)} />)}</div> : <span className="gp-none">No projects in these folders yet.</span>}</>;
}
