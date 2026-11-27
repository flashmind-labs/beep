import { useEffect } from "react";
import { useStore } from "zustand";
import { ensureStash, stash, stashAction, stashAnchor, toggleStash } from "./stash.tsx";

export function StashIndicator({ project }: { project: string | null }) {
  const count = useStore(stash, state => project ? state.entries.get(project)?.length ?? 0 : 0);
  const open = useStore(stash, state => project !== null && state.open === project);
  useEffect(() => { if (project) ensureStash(project); }, [project]);
  if (!project) return null;

  return <span className="prompt-stash-slot" data-stash-project={project}>{count > 0 && <button ref={stashAnchor(project)} className="prompt-stash-count" data-stash-open={project} aria-haspopup="dialog" aria-expanded={open} onClick={e => { e.preventDefault(); toggleStash(project); }}>Stash {count}</button>}</span>;
}

export function StashActions() {
  return <section><b>Stash</b><button type="button" data-stash-action="save" onClick={() => stashAction("save")}><kbd>Ctrl S</kbd>Stash draft</button><button type="button" data-stash-action="pop" onClick={() => stashAction("pop")}><kbd>Ctrl P</kbd>Pop latest</button></section>;
}

/// The key hints under a start composer; `away` while a menu above them is open.
export function StashBar({ project, away = false }: { project: string | null; away?: boolean }) {
  return <div className={`agkeys prompt-stash-bar${away ? " away" : ""}`}><div className="dhints slim"><span><kbd>⇧⌘M</kbd>model</span><span><kbd>esc</kbd>back</span></div><StashIndicator project={project} /><details><summary>Show more</summary><div className="agkeys-list"><StashActions /></div></details></div>;
}
