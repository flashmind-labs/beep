import { useLayoutEffect, useRef } from "react";
import { ago, plural, since } from "../ui/format.ts";
import { IC, PROJECT_COLORS } from "../ui/icons.ts";
import { newWindowOn, openProjects, setProjectOnTop } from "../center/sidebar.ts";
import { barSegments, countsLine, latestLine, workersLabel, type OverviewCard, type OverviewCardRow, type OverviewGroup, type Project } from "./projects-state.ts";
import { editGroup, groupMenu, removeGroup, renameGroup, startRename } from "./groups.ts";
import { state } from "../center/state.ts";
import { redrawProjects } from "./sections.ts";

type Group = OverviewGroup<Project, OverviewCard>;
export function OverviewGroupView({ group, sync }: { group: Group; sync: (name: string) => string | null }) {
  const g = group.group, editing = g && state.renaming?.id === g.id ? state.renaming : null;
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
  </h3>{group.cards.length || !g ? <div className="pj-grid">{group.cards.map(c => <Card key={c.name} card={c} sync={sync(c.name)} />)}</div> : <span className="gp-none">No projects in these folders yet.</span>}</>;
}

function Card({ card: c, sync }: { card: OverviewCardRow<Project, OverviewCard>; sync: string | null }) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (ref.current) newWindowOn(ref.current, ref.current, c.name); }, [c.name]);
  const [open, ...rest] = c.sum ? countsLine(c.sum.tasks) : [];
  return <div ref={ref} className="pj-card" role="button" tabIndex={0} title={c.name} onClick={() => openProjects(c.name)} onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); openProjects(c.name); } }}><div className="h"><span className="cc-pdot" style={{ background: PROJECT_COLORS[c.i % PROJECT_COLORS.length] }} /><b title={c.name}>{c.repo}</b><button type="button" className={`pa${c.pinned ? " on" : ""}`} aria-label={c.pinned ? "Unpin" : "Pin to top"} onClick={e => { e.stopPropagation(); setProjectOnTop(c.name, !c.pinned); }} dangerouslySetInnerHTML={{ __html: IC.pin }} /><small>{c.sum ? workersLabel(c.sum.workers) : c.agents ? plural(c.agents, "agent", "agents") : "idle"}</small></div>
    {c.sum ? <><div className="pj-bar">{barSegments(c.sum.tasks).map(s => <i key={s.state} className={s.state} style={{ width: `${s.share}%` }} title={`${s.n} ${s.state}`} />)}</div><span className="line"><b>{open}</b>{rest.map(x => ` · ${x}`).join("")}</span><span className="line">{latestLine(c.sum.latest)}{c.sum.latest && <small> · {ago(c.sum.latest.updated_at)}</small>}</span></> : <span className="line">{c.last ? `Last activity ${since(c.last)}` : "No recent activity"}</span>}
    {!!c.machines.length && <span className="line sync">on {c.machines.map(m => m.name).join(", ")}</span>}{sync && <span className="line sync">{sync}</span>}</div>;
}
