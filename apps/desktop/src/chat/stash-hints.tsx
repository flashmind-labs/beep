import { useEffect, useSyncExternalStore } from "react";
import { ensureStash, stashAction, stashCount, stashIsOpen, stashRevision, subscribeStash, toggleStash } from "./stash.ts";

export function StashIndicator({ project }: { project: string | null }) {
  useSyncExternalStore(subscribeStash, stashRevision);
  useEffect(() => { if (project) ensureStash(project); }, [project]);
  if (!project) return null;
  const count = stashCount(project);
  return <span className="prompt-stash-slot" data-stash-project={project}>{count > 0 && <button className="prompt-stash-count" data-stash-open={project} aria-haspopup="dialog" aria-expanded={stashIsOpen(project)} onClick={e => { e.preventDefault(); toggleStash(project); }}>Stash {count}</button>}</span>;
}

export function StashActions() {
  return <section><b>Stash</b><button type="button" data-stash-action="save" onClick={() => stashAction("save")}><kbd>Ctrl S</kbd>Stash draft</button><button type="button" data-stash-action="pop" onClick={() => stashAction("pop")}><kbd>Ctrl P</kbd>Pop latest</button></section>;
}

export function StashBar({ project }: { project: string | null }) {
  return <div className="agkeys prompt-stash-bar"><div className="dhints slim"><span><kbd>⌘M</kbd>model</span><span><kbd>esc</kbd>back</span></div><StashIndicator project={project} /><details><summary>Show more</summary><div className="agkeys-list"><StashActions /></div></details></div>;
}
