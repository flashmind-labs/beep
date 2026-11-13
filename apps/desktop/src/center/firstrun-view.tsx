import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { JoinTicketFields, type JoinTicket } from "../machines/jointicket-view.tsx";
import { useDialogRef } from "../ui/parts.ts";
import { ITEMS, ITEM_NAMES, ITEM_ROLES, pending, ready, row, type Item, type Row, type Setup } from "./firstrun-state.ts";
import type { SetupRun } from "./firstrun-setup.ts";

type Step = { kind: "choose" } | { kind: "link" } | { kind: "setup"; run: SetupRun };

export function FirstRunScreen({ done, join, prepare }: { done: () => void; join: JoinTicket; prepare: () => Promise<SetupRun | null> }) {
  const [step, setStep] = useState<Step>({ kind: "choose" });
  const [starting, setStarting] = useState(false);
  const start = useRef<HTMLButtonElement>(null), dialog = useDialogRef();

  useLayoutEffect(() => {
    if (step.kind === "choose") start.current?.focus();
  }, [step]);

  async function begin() {
    setStarting(true);
    const run = await prepare();

    if (!run || ready(run.get())) {
      run?.dispose();
      done();
      return;
    }

    setStep({ kind: "setup", run });
  }

  if (step.kind === "setup") return <SetupScreen run={step.run} done={() => { step.run.dispose(); done(); }} />;

  return <div className="fr-scrim"><div ref={dialog} className="fr-card" role="dialog" aria-label="First run">
    <div className="fr-head">
      <Logo />
      <div><b>{step.kind === "choose" ? "Welcome to Beep" : "Link another desktop"}</b><div className="fr-sub">{step.kind === "choose" ? "Run coding agents on your machines." : "On your other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. This computer then shares its machines, projects and keys."}</div></div>
    </div>
    {step.kind === "choose" ? <>
      <button ref={start} type="button" className="dbtn fr-btn pri" disabled={starting} onClick={() => void begin()}><span className="fr-big"><b>Start</b><span>Use Beep on this computer. You can add machines later.</span></span></button>
      <button type="button" className="dbtn fr-btn " onClick={() => setStep({ kind: "link" })}><span className="fr-big"><b>Link another desktop</b><span>You already use Beep on another computer. Share its machines, projects and keys with this one.</span></span></button>
    </> : <JoinTicketFields done={done} join={join} back={() => setStep({ kind: "choose" })} backLabel="Back" />}
  </div></div>;
}

function SetupScreen({ run, done }: { run: SetupRun; done: () => void }) {
  const s = useSyncExternalStore(run.subscribe, run.get);
  const dialog = useDialogRef();
  const signingIn = ITEMS.some(item => row(s, item).kind === "signing_in");

  // The sign-in sheet opens over this card.
  return <div className={`fr-scrim${signingIn ? " under" : ""}`}><div ref={dialog} className="fr-card fr-setup" role="dialog" aria-label="Set up this computer">
    <div className="fr-head">
      <Logo />
      <div><b>Set up this computer</b><div className="fr-sub">What Beep needs to run coding agents here.</div></div>
    </div>
    <div className="fr-items">{ITEMS.map(item => <SetupRow key={item} item={item} row={row(s, item)} running={s.running} skip={() => run.skip(item)} />)}</div>
    <Footer s={s} start={run.start} done={done} />
  </div></div>;
}

function SetupRow({ item, row: r, running, skip }: { item: Item; row: Row; running: boolean; skip: () => void }) {
  const canSkip = !running && (r.kind === "missing" || r.kind === "signed_out" || r.kind === "failed");

  return <div className="fr-item">
    <div className="fr-item-name"><b>{ITEM_NAMES[item]}</b><span>{ITEM_ROLES[item]}</span></div>
    <div className={`fr-item-state ${tone(r)}`}>
      {(r.kind === "installing" || r.kind === "dialog" || r.kind === "signing_in") && <span className="dspin" />}
      <span>{status(r)}</span>
      {canSkip && <button type="button" className="dbtn quiet small" onClick={skip}>Skip</button>}
    </div>
  </div>;
}

function Footer({ s, start, done }: { s: Setup; start: () => void; done: () => void }) {
  const left = pending(s).filter(item => !s.skipped.has(item)).length;

  if (s.running) return <div className="fr-foot"><button type="button" className="dbtn pri" disabled>Setting up…</button><span className="fr-note">{runningNote(s)}</span><span className="fr-end"><button type="button" className="dbtn quiet" onClick={done}>Later</button></span></div>;
  if (left === 0) return <div className="fr-foot"><button type="button" className="dbtn pri" onClick={done}>Open Beep</button><span className="fr-note">{ready(s) ? "This computer is ready to run agents." : "The rest finishes in the background."}</span></div>;

  return <div className="fr-foot"><button type="button" className="dbtn pri" onClick={start}>Set up</button><span className="fr-note">Installs what is missing, then signs in.</span><span className="fr-end"><button type="button" className="dbtn quiet" onClick={done}>Later</button></span></div>;
}

function runningNote(s: Setup): string {
  const busy = ITEMS.find(item => ["installing", "signing_in"].includes(row(s, item).kind));
  if (!busy) return "";
  return row(s, busy).kind === "installing" ? `Installing ${ITEM_NAMES[busy]}` : `Signing in to ${ITEM_NAMES[busy]}`;
}

function status(r: Row): string {
  switch (r.kind) {
    case "installed": return r.version ? `Installed · v${r.version.replace(/^v/, "")}` : "Installed";
    case "signed_in": return r.account ? `Signed in as ${r.account}` : "Signed in";
    case "signed_out": return "Not signed in";
    case "missing": return "Not installed";
    case "skipped": return "Skipped";
    case "installing": return "Installing";
    case "dialog": return "Finish in the window your system opened";
    case "signing_in": return "Signing in";
    case "failed": return r.reason;
  }
}

function tone(r: Row): string {
  switch (r.kind) {
    case "installed": case "signed_in": return "ok";
    case "failed": return "bad";
    case "signed_out": case "missing": case "skipped": case "installing": case "dialog": case "signing_in": return "";
  }
}

function Logo() {
  return <svg width="34" height="34" viewBox="0 0 28 28" fill="currentColor" aria-hidden="true"><path d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z" /></svg>;
}
