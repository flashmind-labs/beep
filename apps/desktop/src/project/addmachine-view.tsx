import { useLayoutEffect, useRef } from "react";
import type { MachineFolder } from "./projects-state.ts";
import { destination, type CopyLine } from "./addmachine-state.ts";
import { Button } from "../ui/kit.tsx";

interface Props {
  title: string; project: string; machine: string; from: string | null; folder: string; browsing: MachineFolder | null; reading: boolean; adding: boolean; error: string | null; copying: CopyLine | null;
  edit(value: string): void; choose(): void; add(): void; close(): void; browse(path: string | null): void; cancelBrowse(): void; useFolder(listing: MachineFolder): void;
}
export function AddMachineView(p: Props) {
  const input = useRef<HTMLInputElement>(null), wasBrowsing = useRef(false);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  useLayoutEffect(() => {
    if (wasBrowsing.current && !p.browsing) input.current?.focus();
    wasBrowsing.current = !!p.browsing;
  }, [p.browsing]);
  const listing = p.browsing, change = p.from !== null;
  return <><div className="am-head"><b>{p.title}</b><kbd>esc</kbd></div>{p.copying ? <><div className="am-wait"><span className="dspin" />{p.copying.text}</div>{p.copying.pct !== null && <div className="fs-bar am-bar"><i style={{ width: `${p.copying.pct}%` }} /></div>}{p.copying.note && <div className="am-sub">{p.copying.note}</div>}<div className="am-line ip-acts"><Button look="quiet" onClick={p.close}>Close</Button></div></> : <>
    {change && <div className="am-sub">Now in <code>{p.from}</code></div>}<div className="am-label">Folder on {p.machine}</div><div className="am-line"><div className="ip-folder"><input ref={input} className="ip-path ip-dir" type="text" spellCheck={false} autoCapitalize="off" autoCorrect="off" aria-label={`Folder on ${p.machine}`} value={p.folder} title={p.folder} onChange={e => p.edit(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); p.add(); } }} /></div><Button look="quiet" disabled={p.reading || p.adding} onClick={p.choose}>{p.reading ? "Reading…" : "Choose…"}</Button></div>
    {listing && <div className="ip-browse"><div className="at"><Button look="quiet" className="ip-up" disabled={!listing.parent || p.reading} onClick={() => p.browse(listing.parent)}>Up</Button><span title={listing.path}>{listing.path}</span></div><div className="dirs">{listing.folders.map(name => <button type="button" key={name} disabled={p.reading} onClick={() => p.browse(`${listing.path.replace(/\/+$/, "")}/${name}`)}>{name}</button>)}{!listing.folders.length && <span className="none">{listing.exists ? "No folders in here" : "Not there yet"}</span>}{!!listing.more && <span className="none">and {listing.more} more</span>}</div><div className="foot"><span className="note">It goes in {destination(listing, p.project, change)}.</span><Button look="quiet" onClick={p.cancelBrowse}>Cancel</Button><Button look="primary" onClick={() => p.useFolder(listing)}>Use this folder</Button></div></div>}
    {p.error && <div className="am-msg bad">{p.error}</div>}{change && <div className="am-sub">Pick an empty folder, which the other machines fill, or a checkout of the same repository, which joins the sync. The folder it is in now stays on disk as it is.</div>}<div className="am-line ip-acts"><Button look="quiet" onClick={p.close}>Cancel</Button><Button look="primary" className="ip-go" disabled={p.adding || !p.folder.trim()} onClick={p.add}>{p.adding ? (change ? "Changing…" : "Adding…") : change ? "Change" : "Add"}</Button></div>
  </>}</>;
}
