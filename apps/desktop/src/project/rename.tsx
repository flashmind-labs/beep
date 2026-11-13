import { useLayoutEffect, useRef, useState, type FormEvent } from "react";
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
  const field = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { field.current?.focus(); field.current?.select(); }, []);
  const save = async (event: FormEvent) => {
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

  return <form className="pr-form" onSubmit={event => { void save(event); }}>
    <div className="am-head"><b>Rename project</b><kbd>esc</kbd></div>
    <label className="pr-label" htmlFor="project-name">Project name</label>
    <input ref={field} id="project-name" className="gp-field pr-name" value={name} spellCheck={false} maxLength={200} required disabled={busy} onChange={event => setName(event.currentTarget.value)} />
    {project.url && <div className="pr-repository">
      <label className="pr-option">
        <input type="checkbox" checked={renameRepo} disabled={busy} onChange={event => setRenameRepo(event.currentTarget.checked)} />
        <span>Rename repository too<small>{project.url.replace(/^https?:\/\//, "")}</small></span>
      </label>
      {renameRepo && <div className="pr-repository-fields">
        <label className="pr-label" htmlFor="repository-name">Repository name</label>
        <input id="repository-name" className="gp-field" value={repository} disabled={busy} required maxLength={100} spellCheck={false} autoCapitalize="off" pattern="[A-Za-z0-9._-]+" onChange={event => setRepository(event.currentTarget.value)} />
        <p>Git remotes update when machines reconnect. Folders stay unchanged.</p>
      </div>}
    </div>}
    {error !== null && <div className="am-msg bad" role="alert">{error}</div>}
    <div className="gp-foot">
      <button type="button" className="dbtn quiet" disabled={busy} onClick={close}>Cancel</button>
      <button type="submit" className="dbtn pri" disabled={busy || !name.trim() || (name.trim() === projectName(project) && !renameRepo)}>{busy ? "Renaming…" : "Rename"}</button>
    </div>
  </form>;
}
