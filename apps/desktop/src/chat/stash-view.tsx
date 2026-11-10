import type { RefObject } from "react";
import { useInteractOutside } from "react-aria";
import type { StashEntry } from "./stash-state.ts";

export interface StashPanelProps {
  project: string;
  entries: StashEntry[];
  busy: boolean;
  canPop: boolean;
  swap: boolean;
  panelRef: RefObject<HTMLDivElement | null>;
  onClose(): void;
  onRemove(id: string): void;
  onPop(id: string): void;
  onOutside(target: EventTarget | null): void;
}

export function StashPanel({ project, entries, busy, canPop, swap, panelRef, onClose, onRemove, onPop, onOutside }: StashPanelProps) {
  useInteractOutside({ ref: panelRef, onInteractOutside: e => onOutside(e.target) });

  return <div ref={panelRef} className="prompt-stash-panel" role="dialog" aria-label="Project stash"><header><div><b>Stash</b><small>{project}</small></div><button type="button" data-stash-close aria-label="Close stash" autoFocus onClick={onClose}>×</button></header><div className="prompt-stash-items">{entries.map(item => <article key={item.id}><p>{item.text || "Attachments only"}</p><div className="prompt-stash-files">{[...item.images, ...item.files].map((name, index) => <span key={index}>{name}</span>)}</div><footer><button type="button" data-stash-remove={item.id} disabled={busy} onClick={() => onRemove(item.id)}>Remove</button><button type="button" data-stash-pop={item.id} disabled={busy || !canPop} onClick={() => onPop(item.id)}>{swap ? "Stash draft & pop" : "Pop"}</button></footer></article>)}</div></div>;
}
