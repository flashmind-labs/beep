import { useState } from "react";
import { Button, CopyButton, NO_AUTO, SheetHead, Spinner } from "../../ui/kit.tsx";
import { HOME_SEARCH, INSTALL, INSTALL_WINDOWS, KINDS, describeMachine, macOption, timeLeft, withSearch, type AddState, type MacSeen, type TicketKind } from "../../stores/machines/addmachine-state.ts";

interface AddMachineBodyProps {
  state: AddState; kind: TicketKind; mac: MacSeen | null; macAsked: boolean; now: number;
  selectKind: (kind: TicketKind) => void; useThisMac: () => void; start: () => void; close: () => void;
}

export function AddMachineBody(p: AddMachineBodyProps) {
  const [search, setSearch] = useState(HOME_SEARCH);
  const option = p.state.step === "linked" || p.state.step === "paired" ? null : macOption(p.mac, p.macAsked);
  const done = p.state.step === "linked" || p.state.step === "paired" || p.state.step === "expired";
  return <>
    <SheetHead title="Add a machine" />
    {!done && <div className="am-kinds" role="radiogroup" aria-label="Kind of machine">{KINDS.map(k => <Button key={k.kind} className="am-kind" role="radio" aria-checked={p.kind === k.kind} onClick={() => p.selectKind(k.kind)}><b>{k.title}</b><span>{k.sub}</span></Button>)}</div>}
    {option && <div className="am-mac"><div className="am-label">This computer</div>{option.button ? <div className="am-line"><div className="am-mac-text">{option.text}</div><Button look="primary" onClick={p.useThisMac}>{option.button}</Button></div> : option.busy ? <div className="am-wait"><Spinner />{option.text}</div> : <div className={`am-msg${option.bad ? " bad" : ""}`}>{option.done && <><span className="ddot" /> </>}{option.text}</div>}</div>}
    {option && p.state.step !== "linked" && p.state.step !== "paired" && <div className="am-label am-other">Another machine</div>}
    <Body {...p} search={search} setSearch={setSearch} />
  </>;
}

function Body(p: AddMachineBodyProps & { search: string; setSearch: (text: string) => void }) {
  const { state, kind } = p;
  switch (state.step) {
    case "loading": return <div className="am-wait"><Spinner />Making a ticket</div>;
    case "waiting": {
      const status = <div className="am-status am-wait"><Spinner />Waiting for {kind === "phone" ? "the phone" : kind === "desktop" ? "the other desktop" : "the machine"} · works once, <span className="left">{timeLeft(state.expiresAt, p.now)} left</span><span className="end"><Button look="quiet" onClick={p.close}>Cancel</Button></span></div>;
      if (kind === "phone" && state.qrSvg) return <><div className="am-label">Scan with your phone</div><div className="am-phone-qr" aria-label="Phone pairing QR code" dangerouslySetInnerHTML={{ __html: state.qrSvg }} /><div className="am-os">In Beep on your phone, open Linked devices, tap Link workspace, then scan this code. It works once and expires in 5 minutes.</div><details className="am-phone-manual"><summary>Copy ticket instead</summary><CodeLine text={state.command} /></details>{status}</>;
      if (kind === "desktop" || kind === "phone") return <><div className="am-label">Ticket</div><CodeLine text={state.command} /><div className="am-os">{kind === "phone" ? "On your phone, open Devices and paste this ticket. It proves the phone may join, works once and expires in 5 minutes. Keep it private." : "On the other desktop, open Machines, then Link another desktop, and paste this ticket."}</div>{status}</>;
      return <><div className="am-line"><span className="am-label am-search-label">Search for projects in</span><input className="am-code am-search" aria-label="Search for projects in" {...NO_AUTO} value={p.search} onChange={e => p.setSearch(e.currentTarget.value)} /></div><div className="am-label">1. Install beep-node on the machine</div><div className="am-os">macOS or Linux</div><CodeLine text={INSTALL} /><div className="am-os">Windows, in PowerShell</div><CodeLine text={INSTALL_WINDOWS} /><div className="am-label">2. Run this there</div><CodeLine text={withSearch(state.command, p.search)} command />{status}</>;
    }
    case "paired": return <><div className="am-done"><span className="ddot" /><b>Phone paired</b></div><div className="am-sub">Observe projects and agents from your phone. Phones do not host projects or run code, and stay out of Machines.</div><Button look="primary" onClick={p.close}>Done</Button></>;
    case "linked": { const what = describeMachine(state.machine); return <><div className="am-done"><span className="ddot" /><b>{state.machine.name} joined</b></div><div className="am-sub">{what ? `${what}. ` : ""}It shows under Machines and can run agents now.</div><Button look="primary" onClick={p.close}>Done</Button></>; }
    case "expired": return <><div className="am-msg">{kind === "phone" ? "This code expired. Phone codes work once and last 5 minutes." : "This ticket expired. A ticket works once and lasts an hour."}</div><Button look="primary" onClick={p.start}>New ticket</Button></>;
    case "error": return <><div className="am-msg bad">{state.message || "Could not make a ticket."}</div><div className="am-line"><Button look="primary" onClick={p.start}>Try again</Button><Button look="quiet" onClick={p.close}>Close</Button></div></>;
  }
}

function CodeLine({ text, command = false }: { text: string; command?: boolean }) {
  return <div className={`am-line${command ? " am-cmd" : ""}`}><div className="am-code">{text}</div><CopyButton text={text} /></div>;
}
