import { useRef, useState } from "react";
import { flushSync } from "react-dom";
import { invoke } from "../ui/tauri.ts";
import { reason } from "../ui/format.ts";
import { refresh } from "../center/refresh.ts";
import { closeMenu } from "../ui/menu.ts";
import { openSheet } from "../ui/parts.ts";
import { projectName, type Project } from "./projects-state.ts";

export function renameProject(project: Project) {
  closeMenu();
  let busy = false;
  const { close, render } = openSheet("Rename project", "am-sheet project-rename-sheet", { canClose: () => !busy });
  flushSync(() => render(<RenameProject project={project} close={close} pending={value => { busy = value; }} />));
}

function RenameProject({ project, close, pending }: { project: Project; close(): void; pending(value: boolean): void }) {
  const [name, setName] = useState(projectName(project));
  const [renameRepo, setRenameRepo] = useState(false);
  const [repository, setRepository] = useState(project.url?.split("/").filter(Boolean).pop() ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);
  const save = async (event: React.FormEvent) => {
    event.preventDefault();
    if (saving.current) return;
    saving.current = true;
    pending(true); setBusy(true); setError(null);

    try {
      await invoke("rename_project", { project: project.name, name: name.trim(), repository: renameRepo ? repository.trim() : null });
      saving.current = false; pending(false); setBusy(false); close();
      refresh();
    } catch (failure) {
      saving.current = false; pending(false); setBusy(false); setError(reason(failure));
    }
  };

  return <form onSubmit={event => { void save(event); }}>
    <div className="am-head"><b>Rename project</b><kbd>esc</kbd></div>
    <div className="am-label"><label htmlFor="project-name">Project name</label></div>
    <div className="am-line"><input id="project-name" className="gp-field" value={name} autoFocus maxLength={200} required disabled={busy} onChange={event => setName(event.currentTarget.value)} /></div>
    <label className="am-line"><input type="checkbox" checked={renameRepo} disabled={busy || !project.url} onChange={event => setRenameRepo(event.currentTarget.checked)} />Also rename the hosted repository</label>
    <div className="am-label"><label htmlFor="repository-name">Repository name</label></div>
    <div className="am-line"><input id="repository-name" className="gp-field" value={repository} disabled={busy || !renameRepo} required={renameRepo} maxLength={100} pattern="[A-Za-z0-9._-]+" onChange={event => setRepository(event.currentTarget.value)} /></div>
    {project.url && <div className="am-sub">Repository: {project.url}</div>}
    <div className="am-sub">The repository stays with its current owner. Matching Git remotes update when machines reconnect. Local folders keep their paths.</div>
    <div className="am-msg bad" role="alert" hidden={error === null}>{error}</div>
    <div className="gp-foot"><button type="button" className="dbtn quiet" disabled={busy} onClick={close}>Cancel</button><button type="submit" className="dbtn pri" disabled={busy || !name.trim()}>Save</button></div>
  </form>;
}
