import { useEffect, useRef, useState } from "react";
import { copy } from "../ui/parts.ts";
import { stillRunning, type SignInView } from "./signin-state.ts";

interface SignInBodyProps {
  machineName: string;
  tool: string;
  view: SignInView | null;
  error: string;
  sending: boolean;
  send: (text: string) => Promise<void>;
  retry: () => void;
  close: () => void;
  openUrl: (url: string) => void;
}

export function SignInBody({ machineName, tool, view, error, sending, send, retry, close, openUrl }: SignInBodyProps) {
  const [code, setCode] = useState("");
  const field = useRef<HTMLInputElement>(null);
  const mounted = useRef(true);
  const waiting = view?.state === "waiting";
  const asksCode = !!waiting && view.asks_code;
  const over = !!view && !stillRunning(view);

  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => { if (asksCode && !sending) field.current?.focus(); }, [asksCode, sending]);
  async function submit() {
    if (!code.trim() || sending) return;
    await send(code);
    if (mounted.current) setCode("");
  }

  return <>
    <div className="am-head"><b>{`Sign in ${tool} on ${machineName}`}</b><kbd>esc</kbd></div>
    {view?.url && <><div className="am-label">Link</div><div className="am-line"><div className="am-code">{view.url}</div><button type="button" className="dbtn" onClick={e => void copy(view.url ?? "", e.currentTarget)}>Copy</button><button type="button" className="dbtn pri" onClick={() => openUrl(view.url ?? "")}>Open in browser</button></div></>}
    {view?.code && <><div className="am-label">Code to enter there</div><div className="am-line"><div className="am-code">{view.code}</div><button type="button" className="dbtn" onClick={e => void copy(view.code ?? "", e.currentTarget)}>Copy</button></div></>}
    {asksCode && <><div className="am-label">Code from the page</div><div className="am-line"><input ref={field} className="am-code" placeholder="Paste the code" spellCheck={false} autoCapitalize="off" value={code} onChange={e => setCode(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); void submit(); } }} /><button type="button" className="dbtn pri" onClick={() => void submit()}>{sending ? "Sending…" : "Send"}</button></div><div className="am-sub">Sign in on the page, then paste the code it shows.</div></>}
    {(!view || view.state === "starting") && <div className="am-wait"><span className="dspin" />{`Starting ${tool}'s sign-in on ${machineName}…`}</div>}
    {waiting && !view.asks_code && <div className="am-wait"><span className="dspin" />Waiting for you to approve it in the browser</div>}
    {view?.state === "signed_in" && <div className="am-msg">{`${tool} is signed in on ${machineName}.`}</div>}
    {view?.state === "failed" && <div className="am-msg bad">{view.reason || `${tool} could not sign in.`}</div>}
    {view?.state === "idle" && <div className="am-msg">The sign-in was stopped.</div>}
    {error && <div className="am-msg bad">{error}</div>}
    <div className="gp-foot">{over && view?.state === "failed" && <button type="button" className="dbtn" onClick={retry}>Try again</button>}<button type="button" className={over ? "dbtn pri" : "dbtn quiet"} onClick={close}>{over ? "Close" : "Cancel"}</button></div>
  </>;
}
