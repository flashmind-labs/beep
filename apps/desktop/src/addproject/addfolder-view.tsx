import { FolderBrowser } from "./folderbrowser.tsx";
import { useLayoutEffect, useRef } from "react";
import { SyncthingChoice } from "../project/syncthing-choice.tsx";
import type { SyncthingImportChoice } from "../project/syncthing.ts";
import type { MachineFolder } from "../project/projects-state.ts";
import { addable, folderFacts, type FolderState } from "./addfolder-state.ts";
import type { AddFolderOptions } from "./addfolder.tsx";
import { Button, SheetHead } from "../ui/kit.tsx";

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
  return <><SheetHead title="Add a folder" />{state.step === "done" && state.project ? <><div className="am-done"><span className="ddot" /><b>{state.project.name} added</b></div><div className="am-sub">In place on {state.project.machine_name} in {state.project.folder}</div><Button ref={done} look="primary" onClick={p.close}>Open project</Button></> : <>
    <div className="am-label">Machine</div><div className="am-line">{p.machines.some(m => m.online) ? <div className="dchips">{p.machines.filter(m => m.online).map(m => <button type="button" key={m.id} className="dchip" aria-pressed={m.id === state.machine} onClick={() => p.pick(m.id)}>{m.name}</button>)}</div> : <span className="am-sub">No machine is online.</span>}</div>
    <div className="am-label">Folder</div>{listing ? <FolderBrowser listing={listing} reading={p.reading} note={folderFacts(listing, null)[0] ?? null} focusFirst={false} browse={p.browse} cancel={p.cancelBrowse} use={p.use} /> : <div className="am-line"><div className={state.chosen ? "ip-path" : "ip-path dim"} title={state.chosen?.path ?? ""}>{state.chosen?.path ?? "No folder chosen"}</div><Button ref={choose} look="quiet" className="af-choose" disabled={!state.machine || p.reading || state.step !== "form"} onClick={p.choose}>{p.reading ? "Reading…" : "Choose…"}</Button></div>}
    {!!facts.length && <div className="ip-found">{state.chosen?.origin && <span className="ddot" />}{facts[0]}</div>}{facts[1] && <div className="am-sub">{facts[1]}</div>}<SyncthingChoice choice={p.sync} />{state.error && <div className="am-msg bad">{state.error}</div>}<div className="am-line ip-foot"><Button look="primary" className="af-go" disabled={state.step !== "form" || !addable(state)} onClick={p.submit}>{state.step === "adding" ? "Adding…" : "Add project"}</Button><Button look="quiet" onClick={p.back}>Back</Button></div>
  </>}</>;
}
