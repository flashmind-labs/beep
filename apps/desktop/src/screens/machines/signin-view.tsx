import { useEffect, useRef, useState, type ReactNode } from "react";
import { Button, CopyButton, NO_AUTO, SheetHead, Spinner } from "../../ui/kit.tsx";
import { stillRunning, type SignInView } from "../../stores/machines/signin-state.ts";
import { harnessName } from "../../stores/settings/providers-state.ts";

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

  const name = harnessName(tool);

  return <>
    <SheetHead title={`Sign in to ${name}`}><small>{`on ${machineName}`}</small></SheetHead>
    <div className="si-steps">
      {view?.url && <Step title="Open the sign-in page"><div className="si-url" title={view.url}>{shortUrl(view.url)}</div><div className="si-acts"><Button look="primary" onClick={() => openUrl(view.url ?? "")}>Open in browser</Button><CopyButton text={view.url} label="Copy link" look="quiet" /></div></Step>}
      {view?.code && <Step title="Enter this code there"><div className="si-acts"><code className="si-code">{view.code}</code><CopyButton text={view.code} look="quiet" /></div></Step>}
      {asksCode && <Step title={tool === "agy" ? "Paste the code the page shows, within a minute" : "Paste the code the page shows"}><div className="si-acts"><input ref={field} className="am-code" placeholder="Paste the code" aria-label="Code from the page" {...NO_AUTO} value={code} onChange={e => setCode(e.currentTarget.value)} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); void submit(); } }} /><Button look={code.trim() ? "primary" : "secondary"} onClick={() => void submit()}>{sending ? "Sending…" : "Send"}</Button></div></Step>}
    </div>
    {(!view || view.state === "starting") && <div className="am-wait"><Spinner />{`Starting ${name}'s sign-in on ${machineName}…`}</div>}
    {waiting && !view.asks_code && <div className="am-wait"><Spinner />Waiting for you to approve it in the browser</div>}
    {view?.state === "signed_in" && <div className="am-msg">{`${name} is signed in on ${machineName}.`}</div>}
    {view?.state === "failed" && <div className="am-msg bad">{view.reason || `${name} could not sign in.`}</div>}
    {view?.state === "idle" && <div className="am-msg">The sign-in was stopped.</div>}
    {error && <div className="am-msg bad">{error}</div>}
    <div className="gp-foot">{over && view?.state === "failed" && <Button onClick={retry}>Try again</Button>}<Button look={over ? "primary" : "quiet"} onClick={close}>{over ? "Close" : "Cancel"}</Button></div>
  </>;
}

/// One numbered step of the sign-in; the number comes from its place in the list.
function Step({ title, children }: { title: string; children: ReactNode }) {
  return <div className="si-step"><b>{title}</b>{children}</div>;
}

/// The page a link opens, without the query that makes it a dozen lines long.
function shortUrl(url: string) {
  try {
    const { host, pathname } = new URL(url);
    return `${host}${pathname}`;
  } catch {
    return url;
  }
}
