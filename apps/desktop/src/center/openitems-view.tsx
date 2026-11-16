import { useState } from "react";
import { Dropdown } from "../ui/dropdown.tsx";
import { Icon } from "../ui/icons.tsx";
import { RepoSection } from "../project/outside-lists.tsx";
import { GIT_SORTS, parseGitSort, replyStart, projectsWithOpen, type GitItemRow, type GitItemsView, type GitSort } from "./openitems-state.ts";
import { openUrl, quickImport, reviewButton, rowImports } from "./openitems.ts";
import { projectRows, projectWindowProps } from "./sidebar.tsx";
import { state } from "./state.ts";
import { go } from "./views.tsx";

const SORT_KEY = "beep.git.sort";

/// One kind of open item by repository, those with something waiting on the
/// user first. Every repository starts unfolded.
function Column({ kind, title, none, view, sort }: { kind: GitItemRow["kind"]; title: string; none: string; view: GitItemsView; sort: GitSort }) {
  const rows = projectsWithOpen({ items: view.items.filter(item => item.kind === kind) }, projectRows() || []);
  const machines = state.home?.machines?.filter(m => m.can_host !== false);
  const repos = rows.map(row => {
    const importing = rowImports.get(row.key), project = row.project;
    const start = project && { review: (item: GitItemRow) => reviewButton(item, project, machines), reply: (item: GitItemRow) => go("pane", { agentnew: { ...replyStart(item, project, machines), assistant: false } }) };
    return { key: `${kind} ${row.key}`, name: row.repo, failed: importing && importing !== "busy" ? importing.error : null, last: row.last, open: true, items: row.items, start: start || null,
      action: project ? { label: "Open project", windowProps: projectWindowProps(project.name), run: () => go("projects", { project: project.name }) } : { label: importing === "busy" ? "Importing…" : "Import project", disabled: importing === "busy", run: () => { void quickImport(row); } } };
  });
  if (!repos.length) return <div className="cc-empty"><span>{none}</span></div>;
  return <RepoSection id={`section:home-${kind}`} title={title} note="" repos={repos} sort={sort} open={openUrl} />;
}

export function HomeGit({ view }: { view: GitItemsView }) {
  const [sort, setSort] = useState(() => { try { return parseGitSort(localStorage.getItem(SORT_KEY)); } catch { return "default"; } });
  const changeSort = (value: GitSort) => { setSort(value); try { localStorage.setItem(SORT_KEY, value); } catch { /* kept for this launch only */ } };
  return <div className="cc-git" id="sec-git">{[state.gitError, ...view.hosts.map(host => host.error)].filter(Boolean).map((error, index) => <div className="cc-note" key={index}><span className="ddot bad" /><span>{error}</span></div>)}
    <div className="home-sort"><Dropdown label="Sort PRs and issues" options={GIT_SORTS} value={sort} onChange={changeSort} icon={<Icon name="sort"/>}/></div>
    <div className="cc-cols"><Column kind="pull_request" title="Pull requests" none="No open pull requests." view={view} sort={sort} /><Column kind="issue" title="Issues" none="No open issues." view={view} sort={sort} /></div>
  </div>;
}
