import { FolderField } from "./folderbrowser.tsx";
import { useLayoutEffect, useRef } from "react";
import { checkoutNote, forkNote, placeChoices, previewLine, type BranchPicks, type ImportFork, type PreviewView } from "../../stores/review/branchwork.ts";
import { doneText, folderHelp, folderNote, looksLikeRepo, machineChips, progressText, type ImportState } from "../../stores/addproject/importproject-state.ts";
import type { Machine } from "./importproject.tsx";
import type { MachineFolder } from "../../stores/project/projects-state.ts";
import { SyncthingChoice } from "../project/syncthing-choice.tsx";
import type { SyncthingImportChoice } from "../project/syncthing.ts";
import { Button, type ButtonLook, ChipToggle, NO_AUTO, SheetHead, Spinner } from "../../ui/kit.tsx";

interface Props {
  state: ImportState; machines: readonly Machine[]; preview: PreviewView | null | undefined; previewFor: string; previewError: string; fork: ImportFork | null | undefined; picks: BranchPicks;
  folder: string; goes: string; listing: MachineFolder | null; customFolder: boolean; syncChoice: SyncthingImportChoice;
  edit(repo: string): void; pick(machine: string | null): void; submit(): void; close(): void; choose(): void; unchoose(): void; folderInput(value: string): void;
  pickBranch(branch: string, machine: string | null): void;
}
function Folder({ p }: { p: Props }) {
  return <div className="am-line"><FolderField machine={p.state.machine} value={p.folder} label={p.state.chosen ? "Folder" : "Clone into"} placeholder={p.state.machine ? "The machine's projects folder" : "The default folder on the machine Beep picks"} disabled={!p.state.machine} change={p.folderInput} enter={p.submit} /><Button look="quiet" disabled={!p.machines.length} onClick={p.choose}>Choose…</Button>{p.customFolder && <Button look="quiet" onClick={p.unchoose}>Default</Button>}</div>;
}
function Branches({ p }: { p: Props }) {
  if (!p.previewFor || p.previewFor !== p.state.repo.trim()) return null;
  if (p.preview === undefined) return <><div className="am-label">Branches</div><div className="am-wait"><Spinner />Looking for checkouts and branches…</div></>;
  if (p.preview === null) return p.previewError ? <><div className="am-label">Branches</div><div className="am-sub">{p.previewError}</div></> : null;
  const preview = p.preview, choices = placeChoices(preview, p.machines);
  return <><div className="am-label">Branches</div><div className="am-sub">{[checkoutNote(preview), preview.note].filter(Boolean).join(" ")}</div>{preview.branches.length ? <div className="ip-branches">{preview.branches.map(branch => { const meta = previewLine(branch, preview.default_branch); return <div className="ip-branch" title={`${branch.name}  ${meta}`} key={branch.name}><code>{branch.name}</code><span className="m">{meta}</span>{branch.name !== preview.default_branch && !branch.worktree && <><span className="lbl">Worktree</span><span className="pb-seg">{[...choices.map(choice => ({ label: choice.name, value: choice.id as string | null })), { label: "Skip", value: null }].map(choice => <button type="button" key={choice.value ?? "skip"} aria-pressed={choice.value === (p.picks[branch.name] ?? null)} onClick={() => p.pickBranch(branch.name, choice.value)}>{choice.label}</button>)}</span></>}</div>; })}</div> : <div className="am-sub">No branches with work.</div>}</>;
}
function Form(p: Props) {
  const repo = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { const el = repo.current; if (el) { el.focus(); el.setSelectionRange(el.value.length, el.value.length); } }, []);
  const fork = p.previewFor === p.state.repo.trim() && p.listing?.origin == null ? forkNote(p.fork) : null;
  return <><div className="am-label">Repository</div><div className="am-line"><input ref={repo} className="ip-field" type="text" placeholder="org/repo or https://github.com/org/repo" {...NO_AUTO} aria-label="Repository" value={p.state.repo} onChange={e => p.edit(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); p.submit(); } }} /></div><div className="am-label">Machine</div><div className="am-line"><div className="dchips">{machineChips(p.machines).map(chip => <ChipToggle key={chip.id ?? "auto"} label={chip.label} selected={chip.id === p.state.machine} onToggle={() => p.pick(chip.id)} />)}</div></div>
    <div className="am-label">{p.state.chosen ? "Folder" : "Clone into"}</div><Folder p={p} />{p.listing ? <div className="ip-found">{p.listing.origin != null && <span className="ddot" />}{folderNote(p.listing, p.state.repo)}</div> : <div className="am-sub ip-to">{p.goes}</div>}{fork && <div className="ip-found ip-fork"><span className="ddot" />{fork}</div>}<div className="am-sub">{folderHelp(p.state, p.machines, p.customFolder)}</div><Branches p={p} /><SyncthingChoice choice={p.syncChoice} />{p.state.error && <div className="am-msg bad">{p.state.error}</div>}<div className="am-line ip-foot"><Button look="primary" className="ip-go" disabled={!looksLikeRepo(p.state.repo)} onClick={p.submit}>Import</Button><Button look="quiet" onClick={p.close}>Cancel</Button></div></>;
}
function FocusButton({ look, onClick, children }: { look: ButtonLook; onClick(): void; children: string }) {
  const button = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { button.current?.focus(); }, []);
  return <Button ref={button} look={look} onClick={onClick}>{children}</Button>;
}
export function ImportProjectView(p: Props) {
  const state = p.state;
  return <><SheetHead title="Import a project" />{state.step === "cloning" ? <><div className="am-wait"><Spinner />{progressText(state, p.machines, p.listing)}</div><div className="am-sub">A large repository takes a few minutes. You can close this; the project shows up once it is in.</div><div className="am-line"><span className="end" /><FocusButton look="quiet" onClick={p.close}>Close</FocusButton></div></> : state.step === "done" && state.project ? <><div className="am-done"><span className="ddot" /><b>{state.project.name} imported</b></div><div className="am-sub">{doneText(state.project)}</div><FocusButton look="primary" onClick={p.close}>Open project</FocusButton></> : <Form {...p} />}</>;
}
