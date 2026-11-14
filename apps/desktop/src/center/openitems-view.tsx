import { RepoSection } from "../project/outside-lists.tsx";
import { reviewStart, projectsWithOpen, type GitItemRow, type GitItemsView } from "./openitems-state.ts";
import { openUrl, quickImport, replyItem, rowImports } from "./openitems.ts";
import { projectRows } from "./sidebar.tsx";
import { state } from "./state.ts";
import { go } from "./views.tsx";

/// One kind of open item by repository, those with something waiting on the
/// user first. Every repository starts unfolded.
function Column({ kind, title, none, view }: { kind: GitItemRow["kind"]; title: string; none: string; view: GitItemsView }) {
  const rows = projectsWithOpen({ items: view.items.filter(item => item.kind === kind) }, projectRows() || []);
  const projectOf = (item: GitItemRow) => rows.find(row => row.key === `${item.host} ${item.repo}`)?.project ?? null;
  const repos = rows.map(row => {
    const importing = rowImports.get(row.key), project = row.project;
    return { key: `${kind} ${row.key}`, name: row.repo, failed: importing && importing !== "busy" ? importing.error : null, last: row.last, open: true, items: row.items,
      action: project ? { label: "Open project", run: () => go("projects", { project: project.name }) } : { label: importing === "busy" ? "Importing…" : "Import project", disabled: importing === "busy", run: () => { void quickImport(row); } } };
  });
  if (!repos.length) return <div className="cc-empty"><span>{none}</span></div>;
  return <RepoSection id={`section:home-${kind}`} title={title} note="" repos={repos} open={openUrl}
    review={item => go("pane", { agentnew: reviewStart(item, projectOf(item), state.home?.machines?.filter(m => m.can_host !== false)) })}
    reply={item => { void replyItem(item); }} />;
}

export function HomeGit({ view }: { view: GitItemsView }) {
  return <div className="cc-git" id="sec-git">{[state.gitError, ...view.hosts.map(host => host.error)].filter(Boolean).map((error, index) => <div className="cc-note" key={index}><span className="ddot bad" /><span>{error}</span></div>)}
    <div className="cc-cols"><Column kind="pull_request" title="Pull requests" none="No open pull requests." view={view} /><Column kind="issue" title="Issues" none="No open issues." view={view} /></div>
  </div>;
}
