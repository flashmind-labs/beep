import { useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { flushSync } from "react-dom";
import { invoke } from "../../ui/tauri.ts";
import { reason } from "../../ui/format.ts";
import { closeMenu } from "../../ui/menu.tsx";
import { openSheet } from "../../ui/parts.tsx";
import { projectName, type Project } from "../../stores/project/projects-state.ts";
import { Button, Checkbox, NO_AUTO, SheetHead } from "../../ui/kit.tsx";

type RepositoryTarget = { url: string; host: string; name: string };
type RenameResult =
  | { kind: "renamed"; from: string; to: string }
  | { kind: "failed"; url: string; message: string };
type RepositoryChoice = RepositoryTarget & { selected: boolean; result: RenameResult | null };
type RepositoryList =
  | { kind: "loading" }
  | { kind: "ready"; rows: RepositoryChoice[] }
  | { kind: "failed"; message: string };

export function renameProject(project: Project) {
  closeMenu();
  let busy = false;
  const { close, render } = openSheet("Rename project", "am-sheet project-rename-sheet", { canClose: () => !busy });
  flushSync(() => render(<RenameProject project={project} close={close} pending={value => { busy = value; }} />));
}

function RenameProject({ project, close, pending }: { project: Project; close(): void; pending(value: boolean): void }) {
  const [name, setName] = useState(projectName(project));
  const [repositories, setRepositories] = useState<RepositoryList>({ kind: "loading" });
  const [attempt, setAttempt] = useState(0);
  const [savedName, setSavedName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const saving = useRef(false);
  const field = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { field.current?.focus(); field.current?.select(); }, []);
  useEffect(() => {
    let active = true;
    setRepositories({ kind: "loading" });
    void invoke<RepositoryTarget[]>("project_rename_repositories", { project: project.name }).then(rows => {
      if (active) setRepositories({ kind: "ready", rows: rows.map(row => ({ ...row, selected: false, result: null })) });
    }).catch(failure => {
      if (active) setRepositories({ kind: "failed", message: reason(failure) });
    });
    return () => { active = false; };
  }, [project.name, attempt]);

  const rows = repositories.kind === "ready" ? repositories.rows : [];
  const selected = rows.filter(row => row.selected && row.result?.kind !== "renamed");
  const failed = selected.some(row => row.result?.kind === "failed");
  const update = (url: string, change: Partial<Pick<RepositoryChoice, "selected" | "name">>) => {
    setRepositories(current => current.kind === "ready" ? { ...current, rows: current.rows.map(row => row.url === url ? { ...row, ...change } : row) } : current);
  };
  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (saving.current) return;
    saving.current = true;
    pending(true); setBusy(true); setError(null);

    try {
      const results = await invoke<RenameResult[]>("rename_project", {
        project: project.name, name: name.trim(), repositories: selected.map(row => ({ url: row.url, name: row.name.trim() })),
      });
      setSavedName(name.trim());
      setRepositories(current => current.kind === "ready" ? { ...current, rows: current.rows.map(row => {
        const result = results.find(result => (result.kind === "renamed" ? result.from : result.url) === row.url);
        if (!result) return row;
        return result.kind === "renamed" ? { ...row, url: result.to, selected: false, result } : { ...row, result };
      }) } : current);
      saving.current = false; pending(false); setBusy(false);
      if (results.every(result => result.kind === "renamed")) close();
    } catch (failure) {
      saving.current = false; pending(false); setBusy(false); setError(reason(failure));
    }
  };

  return <form className="pr-form" onSubmit={event => { void save(event); }}>
    <SheetHead title="Rename project" />
    <label className="pr-label" htmlFor="project-name">Project name</label>
    <input ref={field} id="project-name" className="gp-field pr-name" value={name} {...NO_AUTO} maxLength={200} required disabled={busy} onChange={event => setName(event.currentTarget.value)} />
    {repositories.kind === "loading" && <p className="pr-loading" role="status">Looking for repository copies…</p>}
    {repositories.kind === "failed" && <div className="pr-host-error" role="alert">{repositories.message} <Button look="quiet" small disabled={busy} onClick={() => setAttempt(attempt + 1)}>Try again</Button></div>}
    {rows.length > 0 && <div className="pr-repositories">
      <div className="pr-label">Also rename repositories</div>
      {rows.map((row, index) => <div className="pr-repository" key={row.url}>
        <label className="pr-option">
          <Checkbox checked={row.selected} disabled={busy || row.result?.kind === "renamed"} change={selected => update(row.url, { selected })} />
          <span>{row.host}<small>{row.url.replace(/^https?:\/\//, "")}</small></span>
          {row.result?.kind === "renamed" && <span className="pr-host-saved">Renamed</span>}
        </label>
        {row.selected && row.result?.kind !== "renamed" && <div className="pr-repository-fields">
          <label className="pr-label" htmlFor={`repository-name-${index}`}>Repository name on {row.host}</label>
          <input id={`repository-name-${index}`} className="gp-field" value={row.name} disabled={busy} required maxLength={100} {...NO_AUTO} pattern="[A-Za-z0-9._-]+" onChange={event => update(row.url, { name: event.currentTarget.value })} />
        </div>}
        {row.result?.kind === "failed" && <div className="pr-host-error" role="alert">{row.result.message}</div>}
      </div>)}
      {selected.length > 0 && <p className="pr-repository-note">Git remotes update when machines reconnect. Folders stay unchanged.</p>}
    </div>}
    {savedName !== null && savedName !== projectName(project) && <p className="pr-loading" role="status">Project name saved.</p>}
    {error !== null && <div className="am-msg bad" role="alert">{error}</div>}
    <div className="gp-foot">
      <Button look="quiet" disabled={busy} onClick={close}>{savedName === null ? "Cancel" : "Close"}</Button>
      <Button type="submit" look="primary" disabled={busy || !name.trim() || (name.trim() === (savedName ?? projectName(project)) && selected.length === 0)}>{busy ? "Renaming…" : failed ? "Retry failed" : "Rename"}</Button>
    </div>
  </form>;
}
