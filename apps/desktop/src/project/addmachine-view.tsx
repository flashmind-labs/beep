import { useLayoutEffect, useRef } from "react";
import type { MachineFolder } from "./projects-state.ts";
import { FolderBrowser } from "../addproject/folderbrowser.tsx";
import { destination, type CopyLine } from "./addmachine-state.ts";

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
  return <><div className="am-head"><b>{p.title}</b><kbd>esc</kbd></div>{p.copying ? <><div className="am-wait"><span className="dspin" />{p.copying.text}</div>{p.copying.pct !== null && <div className="fs-bar am-bar"><i style={{ width: `${p.copying.pct}%` }} /></div>}{p.copying.note && <div className="am-sub">{p.copying.note}</div>}<div className="am-line ip-acts"><button type="button" className="dbtn quiet" onClick={p.close}>Close</button></div></> : <>
    {change && <div className="am-sub">Now in <code>{p.from}</code></div>}<div className="am-label">Folder on {p.machine}</div><div className="am-line"><div className="ip-folder"><input ref={input} className="ip-path ip-dir" type="text" spellCheck={false} autoCapitalize="off" autoCorrect="off" aria-label={`Folder on ${p.machine}`} value={p.folder} title={p.folder} onChange={e => p.edit(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); p.add(); } }} /></div><button type="button" className="dbtn quiet" disabled={p.reading || p.adding} onClick={p.choose}>{p.reading ? "Reading…" : "Choose…"}</button></div>
    {listing && <FolderBrowser listing={listing} reading={p.reading} note={`It goes in ${destination(listing, p.project, change)}.`} focusFirst={false} browse={p.browse} cancel={p.cancelBrowse} use={p.useFolder} />}
    {p.error && <div className="am-msg bad">{p.error}</div>}{change && <div className="am-sub">Pick an empty folder, which the other machines fill, or a checkout of the same repository, which joins the sync. The folder it is in now stays on disk as it is.</div>}<div className="am-line ip-acts"><button type="button" className="dbtn quiet" onClick={p.close}>Cancel</button><button type="button" className="dbtn pri ip-go" disabled={p.adding || !p.folder.trim()} onClick={p.add}>{p.adding ? (change ? "Changing…" : "Adding…") : change ? "Change" : "Add"}</button></div>
  </>}</>;
}
