import type { StashEntry } from "./stash-state.ts";

export function StashPanel({ project, entries, busy, canPop, swap }: { project: string; entries: StashEntry[]; busy: boolean; canPop: boolean; swap: boolean }) {
  return <><header><div><b>Stash</b><small>{project}</small></div><button type="button" data-stash-close aria-label="Close stash">×</button></header><div className="prompt-stash-items">{entries.map(item => <article key={item.id}><p>{item.text || "Attachments only"}</p><div className="prompt-stash-files">{[...item.images, ...item.files].map((name, index) => <span key={index}>{name}</span>)}</div><footer><button type="button" data-stash-remove={item.id} disabled={busy}>Remove</button><button type="button" data-stash-pop={item.id} disabled={busy || !canPop}>{swap ? "Stash draft & pop" : "Pop"}</button></footer></article>)}</div></>;
}
