import { useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { JoinTicketFields, type JoinTicket } from "../machines/jointicket-view.tsx";
import { useDialogRef } from "../../ui/parts.tsx";
import { ITEMS, ITEM_NAMES, ITEM_ROLES, pending, ready, row, type Item, type Row, type Setup } from "../../stores/center/firstrun-state.ts";
import type { SetupRun } from "./firstrun-setup.ts";
import { Button, Spinner } from "../../ui/kit.tsx";
import { SunriseArt } from "../../ui/painted-art.tsx";

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

  return <div className="fr-scrim"><SunriseArt /><div ref={dialog} className="fr-card" role="dialog" aria-label="First run">
    <div className="fr-head">
      <Logo />
      <div><b>{step.kind === "choose" ? "Welcome to Beep" : "Join your other computer"}</b><div className="fr-sub">{step.kind === "choose" ? "Run coding agents on your machines." : "On the computer that already runs Beep, open Machines, then Add a machine, choose Desktop and copy the ticket. This computer then shares its machines, projects and keys."}</div></div>
    </div>
    {step.kind === "choose" ? <div className="fr-ways">
      <Button ref={start} className="fr-way first" disabled={starting} onClick={() => void begin()}><span className="fr-big"><b>Start here</b><span>New to Beep. Set this computer up to run coding agents.</span></span><kbd aria-hidden="true">Enter</kbd></Button>
      <Button className="fr-way" onClick={() => setStep({ kind: "link" })}><span className="fr-big"><b>Join my other computer</b><span>Beep already runs on another computer of yours. Link this one to its machines, projects and keys.</span></span></Button>
    </div> : <JoinTicketFields done={done} join={join} back={() => setStep({ kind: "choose" })} backLabel="Back" />}
  </div></div>;
}

function SetupScreen({ run, done }: { run: SetupRun; done: () => void }) {
  const s = useSyncExternalStore(run.subscribe, run.get);
  const dialog = useDialogRef();
  const signingIn = ITEMS.some(item => row(s, item).kind === "signing_in");

  // The sign-in sheet opens over this card.
  return <div className={`fr-scrim${signingIn ? " under" : ""}`}><SunriseArt /><div ref={dialog} className="fr-card fr-setup" role="dialog" aria-label="Set up this computer">
    <div className="fr-head">
      <Logo />
      <div><b>Set up this computer</b><div className="fr-sub">What Beep needs to run coding agents here.</div></div>
    </div>
    <div className="fr-items">{ITEMS.map(item => <SetupRow key={item} item={item} row={row(s, item)} running={s.running} act={() => run.act(item)} skip={() => run.skip(item)} />)}</div>
    <Footer s={s} start={run.start} done={done} />
  </div></div>;
}

function SetupRow({ item, row: r, running, act, skip }: { item: Item; row: Row; running: boolean; act: () => void; skip: () => void }) {
  const canSkip = !running && (r.kind === "missing" || r.kind === "signed_out" || r.kind === "failed");
  const action = r.kind === "signed_out" ? "Sign in" : r.kind === "missing" ? "Install" : r.kind === "failed" ? "Retry" : null;

  return <div className="fr-item">
    <div className="fr-item-name"><b>{ITEM_NAMES[item]}</b><span>{ITEM_ROLES[item]}</span></div>
    <div className={`fr-item-state ${tone(r)}`}>
      {(r.kind === "installing" || r.kind === "dialog" || r.kind === "signing_in") && <Spinner />}
      <span>{status(r)}</span>
      {canSkip && action && <Button small aria-label={`${action}: ${ITEM_NAMES[item]}`} onClick={act}>{action}</Button>}
      {canSkip && <Button look="quiet" small aria-label={`Skip ${ITEM_NAMES[item]}`} onClick={skip}>Skip</Button>}
    </div>
  </div>;
}

function Footer({ s, start, done }: { s: Setup; start: () => void; done: () => void }) {
  const left = pending(s).filter(item => !s.skipped.has(item)).length;

  if (s.running) return <div className="fr-foot"><Button look="primary" disabled>Setting up…</Button><span className="fr-note">{runningNote(s)}</span><span className="fr-end"><Button look="quiet" onClick={done}>Later</Button></span></div>;
  if (left === 0) return <div className="fr-foot"><Button look="primary" onClick={done}>Open Beep</Button><span className="fr-note">{ready(s) ? "This computer is ready to run agents." : "The rest finishes in the background."}</span></div>;

  return <div className="fr-foot"><Button look="primary" onClick={start}>Set up all</Button><span className="fr-note">Installs what is missing, then signs in.</span><span className="fr-end"><Button look="quiet" onClick={done}>Later</Button></span></div>;
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
