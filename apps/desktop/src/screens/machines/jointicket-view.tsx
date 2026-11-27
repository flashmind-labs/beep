import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Button, NO_AUTO, SheetHead, Spinner } from "../../ui/kit.tsx";

export type JoinTicket = (ticket: string) => Promise<unknown>;

export function JoinTicketFields({ done, join, back, backLabel }: { done: () => void; join: JoinTicket; back: () => void; backLabel: string }) {
  const [ticket, setTicket] = useState("");
  const [status, setStatus] = useState<{ kind: "idle" } | { kind: "joining" } | { kind: "error"; message: string }>({ kind: "idle" });
  const input = useRef<HTMLInputElement>(null);
  const active = useRef(true);
  const pending = useRef(false);
  useLayoutEffect(() => { input.current?.focus(); }, []);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);

  async function submit() {
    if (!ticket.trim() || pending.current) return;
    pending.current = true;
    setStatus({ kind: "joining" });
    try {
      await join(ticket);
      if (active.current) done();
    } catch (error) {
      if (active.current) setStatus({ kind: "error", message: String(error) });
    } finally {
      pending.current = false;
    }
  }

  return <>
    <input ref={input} className="am-code" placeholder="Paste a ticket" {...NO_AUTO} aria-label="Desktop ticket" value={ticket} onChange={e => setTicket(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") void submit(); }} />
    {status.kind === "joining" ? <div className="am-wait"><Spinner />Linking</div> : <div className={status.kind === "error" ? "am-msg bad" : "am-msg"}>{status.kind === "error" ? status.message : ""}</div>}
    <div className="am-line"><Button look="primary" disabled={!ticket.trim() || status.kind === "joining"} onClick={() => void submit()}>Link</Button><Button look="quiet" onClick={back}>{backLabel}</Button></div>
  </>;
}

export function JoinTicketSheet({ done, join }: { done: () => void; join: JoinTicket }) {
  return <><SheetHead title="Link another desktop" />
    <div className="am-os">On the other desktop, open Machines, then Add a machine, choose Desktop and copy the ticket. Paste it here to share its machines, projects and keys.</div>
    <JoinTicketFields done={done} join={join} back={done} backLabel="Cancel" />
  </>;
}
