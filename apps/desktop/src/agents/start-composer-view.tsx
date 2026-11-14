import { SunriseArt } from "../ui/painted-art.tsx";
import { Icon } from "../ui/icons.tsx";
import { Button, Spinner } from "../ui/kit.tsx";
import { createPortal } from "react-dom";
import { Fragment, useLayoutEffect, useMemo, useRef, type ClipboardEvent, type CSSProperties, type ReactNode } from "react";
import { FieldMenu } from "./composer-ui.tsx";

export interface StartComposerNodes { input: HTMLTextAreaElement | null; project: HTMLButtonElement | null; machine: HTMLButtonElement | null; folder: HTMLButtonElement | null; harness: HTMLButtonElement | null; pick: HTMLDivElement | null; att: HTMLDivElement | null; box: HTMLDivElement | null; grow: () => void }
export interface ChoiceContents { project: ReactNode; machine: ReactNode; folder: ReactNode }
/// Which pill's menu or picker is open.
type ComposerOpen = "pick" | "project" | "host" | "folder" | "base" | null;

export interface StartComposerState {
  centered: boolean; busy: boolean; welcome: string;
  /// The line under the field: an error, else the start's progress.
  error: string; progress: string; placeholder: string; invalid: boolean; dropping: boolean;
  sendDisabled: boolean; sendTitle: string | null;
  /// A stash restore holds the send button until it is done.
  stashLocked: boolean;
  harness: ReactNode; harnessDisabled: boolean; open: ComposerOpen;
  projectTitle: string; folderHidden: boolean; folderTitle: string | null;
  choiceView: ReactNode; choiceContents: ChoiceContents | null; attachmentView: ReactNode; uploadPicker: ReactNode;
  notes: ReactNode; footer: ReactNode; under: ReactNode; extras: ReactNode; picker: ReactNode;
}

export interface StartComposerActions {
  submit(): void;
  harness(): void;
  /// A pill of the new-agent form's own choices.
  choice?(which: "project" | "host" | "folder"): void;
  keyDown?(event: KeyboardEvent): void;
  /// The task field changed.
  input?(value: string): void;
  /// Files pasted or dropped on the composer.
  files?(files: File[]): void;
}

const FIELD_MAX = 200;

export function ProjectChoice({ name }: { name: string }) {
  return <><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5" /><rect x="14" y="3" width="7" height="7" rx="1.5" /><rect x="3" y="14" width="7" height="7" rx="1.5" /><rect x="14" y="14" width="7" height="7" rx="1.5" /></svg>{name}<Icon name="caretDown" size={12} /></>;
}
export function FolderChoice({ checkout, label, detail }: { checkout: boolean; label: string; detail: string }) {
  if (checkout) return <>{label} <small>{detail}</small></>;
  return <><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z" /></svg>{label ? <>{label} <small className="mono">{detail}</small></> : <span className="mono">{detail}</span>}</>;
}
export function StartComposerView({ nodes, choices, projectPicker, externalChoices, ids, title, state, actions }: { nodes: StartComposerNodes; choices: HTMLSpanElement; projectPicker: boolean; externalChoices: boolean; ids: boolean; title: string; state: StartComposerState; actions: StartComposerActions }) {
  const row = useRef<HTMLDivElement>(null), box = useRef<HTMLDivElement>(null), input = useRef<HTMLTextAreaElement>(null);
  const id = (value: string) => ids ? value : undefined;
  const grow = () => {
    const field = input.current;
    if (!field?.isConnected) return;
    field.style.height = "auto";
    field.style.height = `${Math.min(field.scrollHeight, FIELD_MAX)}px`;
    field.style.overflowY = field.scrollHeight > FIELD_MAX ? "auto" : "hidden";
  };
  nodes.grow = grow;
  // Stable refs: a callback made per render detaches and reattaches on each
  // draw, so a menu placed from a layout effect would find its pill missing.
  const refs = useMemo(() => ({
    box: (node: HTMLDivElement | null) => { box.current = node; nodes.box = node; },
    att: (node: HTMLDivElement | null) => { nodes.att = node; },
    input: (node: HTMLTextAreaElement | null) => { input.current = node; nodes.input = node; },
    harness: (node: HTMLButtonElement | null) => { nodes.harness = node; },
    pick: (node: HTMLDivElement | null) => { nodes.pick = node; },
    project: (node: HTMLButtonElement | null) => { nodes.project = node; },
    machine: (node: HTMLButtonElement | null) => { nodes.machine = node; },
    folder: (node: HTMLButtonElement | null) => { nodes.folder = node; },
  }), [nodes]);
  // The choices keep one host, so a focused pill or a typed branch stays when the layout changes.
  useLayoutEffect(() => { if (state.centered) box.current?.after(choices); else row.current?.prepend(choices); }, [state.centered, choices]);
  useLayoutEffect(grow, [state.centered]);
  const paste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    const pasted = [...event.clipboardData.items].filter(item => item.kind === "file").map(item => item.getAsFile()).filter((file): file is File => !!file);
    if (!pasted.length || !actions.files) return;
    event.preventDefault();
    event.stopPropagation();
    actions.files(pasted);
  };
  const choice = (which: "project" | "host" | "folder") => () => actions.choice?.(which);
  const expanded = (which: ComposerOpen) => state.open === which;
  const sendLabel = state.centered ? state.busy ? "Starting" : "Start" : undefined;
  return <>{state.centered && <section className="agc-intro"><SunriseArt /><h3 aria-label={state.welcome}><span aria-hidden="true">{state.welcome.split(" ").map((word, index) => <Fragment key={index}>{index > 0 && " "}<span className="agc-word" style={{ "--word": index } as CSSProperties}>{word}</span></Fragment>)}</span></h3><p>A fresh idea. A tricky bug. A place to start.</p></section>}
    <div className={`box${state.invalid ? " err" : ""}${state.dropping ? " dropping" : ""}`} id={id("agbox")} ref={refs.box}>
      <textarea ref={refs.input} id={id("agprompt")} rows={state.centered ? 3 : 1} autoComplete="off" spellCheck={false} aria-label="Task for a coding agent" title={title} placeholder={state.placeholder}
        onInput={event => { grow(); actions.input?.(event.currentTarget.value); }}
        onPaste={paste}
        onKeyDown={event => { actions.keyDown?.(event.nativeEvent); if (event.currentTarget.readOnly || event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing || event.defaultPrevented) return; event.preventDefault(); event.stopPropagation(); event.currentTarget.form?.requestSubmit(); }} />
      <div className="row" ref={row}><span className="sp" /><button ref={refs.harness} id={id("agpillb")} className="pill" type="button" title="Harness, model and thinking" disabled={state.harnessDisabled} aria-expanded={expanded("pick")} onClick={actions.harness}>{state.harness}</button>{state.uploadPicker}<div className="ag-att" hidden={!state.attachmentView} ref={refs.att}>{state.attachmentView}</div><button id={id("aggo")} className={state.centered ? "agc-send" : "dbtn pri"} type="submit" disabled={state.sendDisabled || state.stashLocked} title={state.sendTitle ?? undefined} aria-label={sendLabel}>{state.centered ? state.busy ? <Spinner /> : <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M12 19V5M5 12l7-7 7 7" /></svg> : <>{state.busy ? "Starting" : "Start"}<kbd>⏎</kbd></>}</button></div>
      <div className="pick" id={id("agpick")} ref={refs.pick}>{state.picker}</div>{state.under}<FieldMenu /></div>
    <span className={state.error ? "cc-err" : "cc-err cc-progress"} id={id("agerr")} role="status">{state.error || state.progress}</span>
    {state.notes}
    {state.footer}
    {createPortal(externalChoices ? state.choiceView : <>{projectPicker && <button ref={refs.project} id={id("agprojb")} className="pill" type="button" title={state.projectTitle} aria-expanded={expanded("project")} onClick={choice("project")}>{state.choiceContents?.project}</button>}<button ref={refs.machine} id={id("aghostb")} className="pill" type="button" title="The machine it runs on" aria-expanded={expanded("host")} onClick={choice("host")}>{state.choiceContents?.machine}</button><button ref={refs.folder} id={id("agfoldb")} className="pill" type="button" title={state.folderTitle ?? "The checkout, a new worktree, or an existing worktree"} hidden={state.folderHidden} aria-expanded={expanded("folder")} onClick={choice("folder")}>{state.choiceContents?.folder}</button><span className="choices">{state.extras}</span></>, choices)}
  </>;
}

export function NewAgentNotes({ waiting, none, noMachine, openMachines }: { waiting: boolean; none: boolean; noMachine: boolean; openMachines(): void }) {
  return <div className="agnew-notes"><div className="agc-why" id="agwhy" hidden={!noMachine}>{noMachine && <><span className="ddot off" /><span>Pick a machine to start.</span></>}</div><div id="aghelp">{waiting ? <div className="agc-note"><Spinner /> Looking for machines…</div> : none ? <div className="agc-help"><b>No machine can run an agent yet</b><ol><li>Install beep-node on a computer that has Claude Code, Codex, Gemini CLI, pi or flsh.</li><li>Open Add a machine and run the <code>beep-node mesh join</code> command it shows there.</li></ol><div><Button id="agmachines" onClick={openMachines}>Machines settings</Button></div></div> : null}</div></div>;
}
