import { useLayoutEffect, useRef } from "react";
import { plural } from "../ui/format.ts";
import { newWindowOn, openProjects, setProjectOnTop } from "../center/sidebar.ts";
import { type OverviewCard, type OverviewGroup, type Project } from "./projects-state.ts";
import { OverviewCardView, overviewCardModel } from "./overview-card.tsx";
import { editGroup, groupMenu, removeGroup, renameGroup, startRename } from "./groups.ts";
import { state } from "../center/state.ts";
import { redrawProjects } from "./sections.ts";

type Group = OverviewGroup<Project, OverviewCard>;
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
