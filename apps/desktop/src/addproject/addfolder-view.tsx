import { useLayoutEffect, useRef } from "react";
import { SyncthingChoice } from "../project/syncthing-choice.tsx";
import type { SyncthingImportChoice } from "../project/syncthing.ts";
import type { MachineFolder } from "../project/projects-state.ts";
import { addable, folderFacts, type FolderState } from "./addfolder-state.ts";
import type { AddFolderOptions } from "./addfolder.ts";

interface Props {
  state: FolderState; machines: NonNullable<AddFolderOptions["machines"]>; reading: boolean; sync: SyncthingImportChoice;
  pick(id: string): void; choose(): void; browse(path: string | null): void; cancelBrowse(): void; use(listing: MachineFolder): void; submit(): void; back(): void; close(): void;
}
export function AddFolderView(p: Props) {
  const { state } = p, listing = state.browsing, facts = folderFacts(state.chosen, state.branch);
  const done = useRef<HTMLButtonElement>(null), choose = useRef<HTMLButtonElement>(null), browsing = useRef(!!listing);
  useLayoutEffect(() => {
    if (browsing.current && !listing) choose.current?.focus();
    browsing.current = !!listing;
  }, [listing]);
  useLayoutEffect(() => { if (state.step === "done") done.current?.focus(); }, [state.step]);
  return <><div className="am-head"><b>Add a folder</b><kbd>esc</kbd></div>{state.step === "done" && state.project ? <><div className="am-done"><span className="ddot" /><b>{state.project.name} added</b></div><div className="am-sub">In place on {state.project.machine_name} in {state.project.folder}</div><button ref={done} type="button" className="dbtn pri" onClick={p.close}>Open project</button></> : <>
    <div className="am-label">Machine</div><div className="am-line">{p.machines.some(m => m.online) ? <div className="dchips">{p.machines.filter(m => m.online).map(m => <button type="button" key={m.id} className="dchip" aria-pressed={m.id === state.machine} onClick={() => p.pick(m.id)}>{m.name}</button>)}</div> : <span className="am-sub">No machine is online.</span>}</div>
    <div className="am-label">Folder</div>{listing ? <div className="ip-browse"><div className="at"><button type="button" className="dbtn quiet ip-up" disabled={!listing.parent || p.reading} onClick={() => p.browse(listing.parent)}>Up</button><span title={listing.path}>{listing.path}</span></div><div className="dirs">{listing.folders.map(name => <button type="button" key={name} disabled={p.reading} onClick={() => p.browse(`${listing.path.replace(/\/+$/, "")}/${name}`)}>{name}</button>)}{!listing.folders.length && <span className="none">{listing.exists ? "No folders in here" : "Not there yet"}</span>}{!!listing.more && <span className="none">and {listing.more} more</span>}</div><div className="foot"><span className="note">{folderFacts(listing, null)[0] ?? null}</span><button type="button" className="dbtn quiet" onClick={p.cancelBrowse}>Cancel</button><button type="button" className="dbtn pri" onClick={() => p.use(listing)}>Use this folder</button></div></div> : <div className="am-line"><div className={state.chosen ? "ip-path" : "ip-path dim"} title={state.chosen?.path ?? ""}>{state.chosen?.path ?? "No folder chosen"}</div><button ref={choose} type="button" className="dbtn quiet af-choose" disabled={!state.machine || p.reading || state.step !== "form"} onClick={p.choose}>{p.reading ? "Reading…" : "Choose…"}</button></div>}
    {!!facts.length && <div className="ip-found">{state.chosen?.origin && <span className="ddot" />}{facts[0]}</div>}{facts[1] && <div className="am-sub">{facts[1]}</div>}<SyncthingChoice choice={p.sync} />{state.error && <div className="am-msg bad">{state.error}</div>}<div className="am-line ip-foot"><button type="button" className="dbtn pri af-go" disabled={state.step !== "form" || !addable(state)} onClick={p.submit}>{state.step === "adding" ? "Adding…" : "Add project"}</button><button type="button" className="dbtn quiet" onClick={p.back}>Back</button></div>
  </>}</>;
}
