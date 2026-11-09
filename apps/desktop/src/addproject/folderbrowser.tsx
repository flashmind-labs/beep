import { useLayoutEffect, useRef } from "react";
import type { MachineFolder } from "../project/projects-state.ts";

interface Props {
  listing: MachineFolder; reading: boolean; note: string | null; focusFirst: boolean;
  browse(path: string | null): void; cancel(): void; use(listing: MachineFolder): void;
}

/// Shared remote folder controls; each sheet owns its navigation and selection.
export function FolderBrowser(p: Props) {
  const { listing } = p, first = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { if (p.focusFirst) first.current?.focus(); }, [listing.path, p.focusFirst]);
  const into = (name: string) => `${listing.path.replace(/\/+$/, "")}/${name}`;
  return <div className="ip-browse"><div className="at"><button type="button" className="dbtn quiet ip-up" disabled={!listing.parent || p.reading} onClick={() => p.browse(listing.parent)}>Up</button><span title={listing.path}>{listing.path}</span></div>
    <div className="dirs">{listing.folders.map((name, index) => <button type="button" ref={index === 0 ? first : null} key={name} disabled={p.reading} onClick={() => p.browse(into(name))}>{name}</button>)}{!listing.folders.length && <span className="none">{listing.exists ? "No folders in here" : "Not there yet"}</span>}{!!listing.more && <span className="none">and {listing.more} more</span>}</div>
    <div className="foot"><span className="note">{p.note}</span><button type="button" className="dbtn quiet" onClick={p.cancel}>Cancel</button><button type="button" className="dbtn pri" onClick={() => p.use(listing)}>Use this folder</button></div>
  </div>;
}
