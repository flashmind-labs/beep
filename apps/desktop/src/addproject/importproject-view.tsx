import { useLayoutEffect, useRef, type KeyboardEvent } from "react";
import { checkoutNote, forkNote, placeChoices, previewLine, type BranchPicks, type ImportFork, type PreviewView } from "../review/branchwork.ts";
import type { MachineFolder } from "../project/projects-state.ts";
import { doneText, folderHelp, folderNote, looksLikeRepo, machineChips, progressText, type ImportState } from "./importproject-state.ts";
import { completed, isOpen, type FolderMenu } from "./foldercomplete.ts";
import type { Machine } from "./importproject.ts";

interface Props {
  state: ImportState; machines: readonly Machine[]; preview: PreviewView | null | undefined; previewFor: string; previewError: string; fork: ImportFork | null | undefined; picks: BranchPicks;
  reading: boolean; folder: string; goes: string; menu: FolderMenu; syncNode: HTMLElement;
  edit(repo: string): void; pick(machine: string | null): void; submit(): void; close(): void; choose(): void; unchoose(): void; folderInput(value: string): void; folderKey(event: KeyboardEvent<HTMLInputElement>): void; folderBlur(): void;
  acceptFolder(text: string): void; browse(path: string | null): void; cancelBrowse(): void; useFolder(listing: MachineFolder): void; pickBranch(branch: string, machine: string | null): void;
}
function SyncChoice({ node }: { node: HTMLElement }) { const host = useRef<HTMLDivElement>(null); useLayoutEffect(() => { const el = host.current; if (!el) return; el.append(node); return () => { node.remove(); }; }, [node]); return <div ref={host} style={{ display: "contents" }} />; }
function Browser({ listing, p }: { listing: MachineFolder; p: Props }) {
  const first = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { first.current?.focus(); }, [listing.path]);
  return <div className="ip-browse"><div className="at"><button type="button" className="dbtn quiet ip-up" disabled={!listing.parent || p.reading} onClick={() => p.browse(listing.parent)}>Up</button><span title={listing.path}>{listing.path}</span></div><div className="dirs">{listing.folders.map((name, index) => <button type="button" ref={index === 0 ? first : null} key={name} disabled={p.reading} onClick={() => p.browse(`${listing.path.replace(/\/+$/, "")}/${name}`)}>{name}</button>)}{!listing.folders.length && <span className="none">{listing.exists ? "No folders in here" : "Not there yet"}</span>}{!!listing.more && <span className="none">and {listing.more} more</span>}</div><div className="foot"><span className="note">{folderNote(listing, p.state.repo)}</span><button type="button" className="dbtn quiet" onClick={p.cancelBrowse}>Cancel</button><button type="button" className="dbtn pri" onClick={() => p.useFolder(listing)}>Use this folder</button></div></div>;
}
function Folder({ p }: { p: Props }) {
  const active = useRef<HTMLDivElement>(null), opened = isOpen(p.menu);
  useLayoutEffect(() => { if (opened) active.current?.scrollIntoView({ block: "nearest" }); }, [opened, p.menu.index, p.menu.dir]);
  return <div className="am-line"><div className="ip-folder"><input className="ip-path ip-dir" type="text" spellCheck={false} autoCapitalize="off" autoCorrect="off" aria-label={p.state.chosen ? "Folder" : "Clone into"} role="combobox" aria-autocomplete="list" aria-controls="ip-menu" aria-expanded={opened} aria-activedescendant={opened ? `ip-menu-${p.menu.index}` : undefined} placeholder={p.state.machine ? "The machine's projects folder" : "The default folder on the machine Beep picks"} disabled={!p.state.machine} value={p.folder} title={p.folder} onChange={e => p.folderInput(e.currentTarget.value)} onKeyDown={p.folderKey} onBlur={p.folderBlur} />
    {opened && <div className="ip-menu" id="ip-menu" role="listbox">{p.menu.items.map((name, i) => <div key={name} ref={i === p.menu.index ? active : null} id={`ip-menu-${i}`} className={i === p.menu.index ? "on" : ""} role="option" aria-selected={i === p.menu.index} onMouseDown={e => e.preventDefault()} onClick={() => p.acceptFolder(completed(p.menu.dir, name))}>{name}</div>)}</div>}</div><button type="button" className="dbtn quiet" disabled={!p.machines.length || p.reading} onClick={p.choose}>{p.reading ? "Reading…" : "Choose…"}</button>{p.state.chosen && <button type="button" className="dbtn quiet" onClick={p.unchoose}>Default</button>}</div>;
}
function Branches({ p }: { p: Props }) {
  if (!p.previewFor || p.previewFor !== p.state.repo.trim()) return null;
  if (p.preview === undefined) return <><div className="am-label">Branches</div><div className="am-wait"><span className="dspin" />Looking for checkouts and branches…</div></>;
  if (p.preview === null) return p.previewError ? <><div className="am-label">Branches</div><div className="am-sub">{p.previewError}</div></> : null;
  const preview = p.preview, choices = placeChoices(preview, p.machines);
  return <><div className="am-label">Branches</div><div className="am-sub">{[checkoutNote(preview), preview.note].filter(Boolean).join(" ")}</div>{preview.branches.length ? <div className="ip-branches">{preview.branches.map(branch => { const meta = previewLine(branch, preview.default_branch); return <div className="ip-branch" title={`${branch.name}  ${meta}`} key={branch.name}><code>{branch.name}</code><span className="m">{meta}</span>{branch.name !== preview.default_branch && !branch.worktree && <><span className="lbl">Worktree</span><span className="pb-seg">{[...choices.map(choice => ({ label: choice.name, value: choice.id as string | null })), { label: "Skip", value: null }].map(choice => <button type="button" key={choice.value ?? "skip"} aria-pressed={choice.value === (p.picks[branch.name] ?? null)} onClick={() => p.pickBranch(branch.name, choice.value)}>{choice.label}</button>)}</span></>}</div>; })}</div> : <div className="am-sub">No branches with work.</div>}</>;
}
function Form(p: Props) {
  const repo = useRef<HTMLInputElement>(null);
  const browsing = useRef(!!p.state.browsing);
  useLayoutEffect(() => { const el = repo.current; if (el) { el.focus(); el.setSelectionRange(el.value.length, el.value.length); } }, []);
  useLayoutEffect(() => {
    if (browsing.current && !p.state.browsing) repo.current?.focus();
    browsing.current = !!p.state.browsing;
  }, [p.state.browsing]);
  const fork = p.previewFor === p.state.repo.trim() && p.state.chosen?.origin == null ? forkNote(p.fork) : null;
  return <><div className="am-label">Repository</div><div className="am-line"><input ref={repo} className="ip-field" type="text" placeholder="org/repo or https://github.com/org/repo" spellCheck={false} autoCapitalize="off" autoCorrect="off" aria-label="Repository" value={p.state.repo} onChange={e => p.edit(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); p.submit(); } }} /></div><div className="am-label">Machine</div><div className="am-line"><div className="dchips">{machineChips(p.machines).map(chip => <button type="button" className="dchip" key={chip.id ?? "auto"} aria-pressed={chip.id === p.state.machine} onClick={() => p.pick(chip.id)}>{chip.label}</button>)}</div></div>
    <div className="am-label">{p.state.chosen ? "Folder" : "Clone into"}</div>{p.state.browsing ? <Browser listing={p.state.browsing} p={p} /> : <Folder p={p} />}{p.state.chosen && !p.state.browsing && <div className="ip-found">{p.state.chosen.origin != null && <span className="ddot" />}{folderNote(p.state.chosen, p.state.repo)}</div>}{!p.state.chosen && !p.state.browsing && <div className="am-sub ip-to">{p.goes}</div>}{fork && <div className="ip-found ip-fork"><span className="ddot" />{fork}</div>}<div className="am-sub">{folderHelp(p.state, p.machines)}</div><Branches p={p} /><SyncChoice node={p.syncNode} />{p.state.error && <div className="am-msg bad">{p.state.error}</div>}<div className="am-line ip-foot"><button type="button" className="dbtn pri ip-go" disabled={!looksLikeRepo(p.state.repo)} onClick={p.submit}>Import</button><button type="button" className="dbtn quiet" onClick={p.close}>Cancel</button></div></>;
}
function FocusButton({ className, onClick, children }: { className: string; onClick(): void; children: string }) {
  const button = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { button.current?.focus(); }, []);
  return <button ref={button} type="button" className={className} onClick={onClick}>{children}</button>;
}
export function ImportProjectView(p: Props) {
  const state = p.state;
  return <><div className="am-head"><b>Import a project</b><kbd>esc</kbd></div>{state.step === "cloning" ? <><div className="am-wait"><span className="dspin" />{progressText(state, p.machines)}</div><div className="am-sub">A large repository takes a few minutes. You can close this; the project shows up once it is in.</div><div className="am-line"><span className="end" /><FocusButton className="dbtn quiet" onClick={p.close}>Close</FocusButton></div></> : state.step === "done" && state.project ? <><div className="am-done"><span className="ddot" /><b>{state.project.name} imported</b></div><div className="am-sub">{doneText(state.project)}</div><FocusButton className="dbtn pri" onClick={p.close}>Open project</FocusButton></> : <Form {...p} />}</>;
}
