import { useState } from "react";
import { ChipToggle } from "../ui/kit.tsx";
import { RepoSection } from "../project/outside-lists.tsx";
import { replyStart, reviewStart, projectsWithOpen, type GitItemRow, type GitItemsView } from "./openitems-state.ts";
import { openUrl, quickImport, rowImports } from "./openitems.ts";
import { projectRows } from "./sidebar.tsx";
import { state } from "./state.ts";
import { go } from "./views.tsx";

/// Whether Home lists open items newest first across repositories, instead of by repository.
const NEWEST_KEY = "beep.gitNewest";

/// One kind of open item by repository, those with something waiting on the
/// user first. Every repository starts unfolded.
function Column({ kind, title, none, view, newest }: { kind: GitItemRow["kind"]; title: string; none: string; view: GitItemsView; newest: boolean }) {
  const rows = projectsWithOpen({ items: view.items.filter(item => item.kind === kind) }, projectRows() || []);
  const machines = state.home?.machines?.filter(m => m.can_host !== false);
  const repos = rows.map(row => {
    const importing = rowImports.get(row.key), project = row.project;
    const start = project && { review: (item: GitItemRow) => go("pane", { agentnew: { ...reviewStart(item, project, machines), assistant: false } }), reply: (item: GitItemRow) => go("pane", { agentnew: { ...replyStart(item, project, machines), assistant: false } }) };
    return { key: `${kind} ${row.key}`, name: row.repo, failed: importing && importing !== "busy" ? importing.error : null, last: row.last, open: true, items: row.items, start: start || null,
      action: project ? { label: "Open project", run: () => go("projects", { project: project.name }) } : { label: importing === "busy" ? "Importing…" : "Import project", disabled: importing === "busy", run: () => { void quickImport(row); } } };
  });
  if (!repos.length) return <div className="cc-empty"><span>{none}</span></div>;
  return <RepoSection id={`section:home-${kind}`} title={title} note="" repos={repos} newest={newest} open={openUrl} />;
}

export function HomeGit({ view }: { view: GitItemsView }) {
  const [newest, setNewest] = useState(() => { try { return localStorage.getItem(NEWEST_KEY) === "1"; } catch { return false; } });
  const toggle = () => { setNewest(!newest); try { localStorage.setItem(NEWEST_KEY, newest ? "0" : "1"); } catch { /* kept for this launch only */ } };
  return <div className="cc-git" id="sec-git">{[state.gitError, ...view.hosts.map(host => host.error)].filter(Boolean).map((error, index) => <div className="cc-note" key={index}><span className="ddot bad" /><span>{error}</span></div>)}
    <div><ChipToggle label="Newest first" selected={newest} onToggle={toggle} /></div>
    <div className="cc-cols"><Column kind="pull_request" title="Pull requests" none="No open pull requests." view={view} newest={newest} /><Column kind="issue" title="Issues" none="No open issues." view={view} newest={newest} /></div>
  </div>;
}
