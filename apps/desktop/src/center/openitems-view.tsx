import { useState } from "react";
import { Dropdown } from "../ui/dropdown.tsx";
import { Icon } from "../ui/icons.tsx";
import { Button } from "../ui/kit.tsx";
import { Segmented } from "../ui/aria-kit.tsx";
import { RepoCards, type RepoLine } from "../project/outside-lists.tsx";
import { GIT_SORTS, dayLabel, inPeriod, itemTime, parseGitSort, projectsWithOpen, type GitItemRow, type GitItemsView, type GitPeriod, type GitSort } from "./openitems-state.ts";
import { continueButton, itemMenu, quickImport, reviewButton, rowImports } from "./openitems.ts";
import { openUrl } from "../ui/tauri.ts";
import { projectRows } from "../project/projectrows.ts";
import { projectWindowProps } from "../project/projectops.ts";
import { state } from "./state.ts";
import { go } from "./nav.ts";

const SORT_KEY = "beep.git.sort";

/// The period Home's open items show, kept while the user is on other pages.
let keptPeriod: GitPeriod = { kind: "all" };

/// Home's open pull requests and issues, a card per repository: all of them, or those that
/// changed this week or on one day, which the arrows step back and forward.
export function HomeGit({ view }: { view: GitItemsView }) {
  const [sort, setSort] = useState(() => { try { return parseGitSort(localStorage.getItem(SORT_KEY)); } catch { return "default"; } });
  const changeSort = (value: GitSort) => { setSort(value); try { localStorage.setItem(SORT_KEY, value); } catch { /* kept for this launch only */ } };
  const [period, setPeriod] = useState(keptPeriod);
  const changePeriod = (next: GitPeriod) => { keptPeriod = next; setPeriod(next); };
  const now = Date.now(), back = period.kind === "day" ? period.back : 0;
  const periods = [{ id: "all", label: "All" }, { id: "week", label: "This week" }, { id: "day", label: dayLabel(back, now) }] as const;
  const items = inPeriod(view.items, period, sort, now);

  return <div className="cc-git" id="sec-git">{[state.gitError, ...view.hosts.map(host => host.error)].filter(Boolean).map((error, index) => <div className="cc-note" key={index}><span className="ddot bad" /><span>{error}</span></div>)}
    <div className="home-sort">
      <Segmented label="Period" options={periods} value={period.kind} onChange={kind => changePeriod(kind === "day" ? { kind, back } : { kind })}/>
      {period.kind === "day" && <>
        <Button look="quiet" small aria-label="Previous day" onClick={() => changePeriod({ kind: "day", back: back + 1 })}><Icon name="back" size={12}/></Button>
        <Button look="quiet" small aria-label="Next day" disabled={back === 0} onClick={() => changePeriod({ kind: "day", back: back - 1 })}><Icon name="chev" size={12}/></Button>
      </>}
      <span className="sp"/>
      <Dropdown label="Sort PRs and issues" options={GIT_SORTS} value={sort} onChange={changeSort} icon={<Icon name="sort"/>}/>
    </div>
    {items.length ? <RepoCards repos={repoLines({ items }, sort)} sort={sort} open={openUrl} />
      : <div className="cc-empty"><span>{period.kind === "all" ? "No open pull requests or issues." : "No pull request or issue changed in that time."}</span></div>}
  </div>;
}

/// Every repository with something open, those with something waiting on the user first, then
/// by its latest item as `sort` times it.
function repoLines(view: Pick<GitItemsView, "items">, sort: GitSort): RepoLine[] {
  const machines = state.home?.machines?.filter(m => m.can_host !== false);
  const latest = (items: readonly GitItemRow[]) => Math.max(...items.map(item => itemTime(sort, item)));
  const rows = projectsWithOpen(view, projectRows() || []);
  if (sort !== "default") rows.sort((a, b) => latest(b.items) - latest(a.items));

  return rows.map(row => {
    const importing = rowImports.get(row.key), project = row.project;
    const start = project && { continue: (item: GitItemRow) => continueButton(item, project, machines), review: (item: GitItemRow) => reviewButton(item, project, machines), menu: (item: GitItemRow) => itemMenu(item, project, machines) };
    return { key: row.key, name: row.repo, failed: importing && importing !== "busy" ? importing.error : null, last: row.last, open: true, items: row.items, start: start || null,
      action: project ? { label: "Open project", windowProps: projectWindowProps(project.name), run: () => go("projects", { project: project.name }) } : { label: importing === "busy" ? "Importing…" : "Import project", disabled: importing === "busy", run: () => { void quickImport(row); } } };
  });
}
