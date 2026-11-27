import { Button, CopyButton, Spinner, SheetHead } from "../../ui/kit.tsx";
import { PERIODS, answerLabel, canSend, issueHint, sizeLabel, type ReportEvent, type ReportState } from "../../stores/report/report-state.ts";

interface ReportBodyProps {
  state: ReportState;
  title: string;
  opened: string | null;
  issueText: string | null;
  debug: boolean;
  update: (event: ReportEvent) => void;
  collect: () => void;
  send: () => void;
  back: () => void;
  retry: (button: HTMLButtonElement) => void;
  openIssue: (button: HTMLButtonElement) => void;
  cancel: () => void;
  done: () => void;
  startDebug: () => void;
  openFile: (path: string) => void;
  openUrl: () => void;
}

export function ReportBody(props: ReportBodyProps) {
  return <>
    <SheetHead title={props.title} />
    <Body {...props} />
  </>;
}

function Body(p: ReportBodyProps) {
  const { state } = p;
  switch (state.step) {
    case "form":
    case "collecting": {
      const collecting = state.step === "collecting";
      return <>
        <div className="am-label">What happened?</div>
        <textarea className="rp-ta" autoComplete="off" autoCorrect="off" placeholder="What happened?" aria-label="What happened?" value={state.text} disabled={collecting} onChange={e => p.update({ type: "text", text: e.currentTarget.value })} />
        <div className="dchips rp-periods" role="radiogroup" aria-label="Period">{PERIODS.map(({ period, label }) =>
          <button key={period} type="button" className="dchip" role="radio" aria-checked={state.period === period} aria-pressed={state.period === period} disabled={collecting} onClick={() => p.update({ type: "period", period })}>{label}</button>)}</div>
        {collecting && <div className="am-wait rp-wait"><Spinner />Collecting from your machines</div>}
        <div className="rp-foot"><Button look="quiet" disabled={collecting} onClick={p.cancel}>Cancel</Button>
          {p.debug && <Button title="Starts a coding agent that looks into Beep and sends a report" disabled={collecting} onClick={p.startDebug}>Debug with an agent</Button>}
          <Button look="primary" disabled={collecting} onClick={p.collect}>Collect</Button></div>
      </>;
    }
    case "files":
    case "sending": {
      const draft = state.draft;
      if (!draft) return null;
      const bytes = new Map(draft.manifest.files.map(f => [f.path, f.bytes]));
      const shown = draft.files.find(f => f.path === p.opened);
      const total = draft.manifest.files.reduce((n, f) => n + f.bytes, 0);
      return <>
        <div className="am-label">Machines</div>
        <div className="rp-box">{draft.manifest.machines.map(m => <div className="rp-row" key={m.id}><span>{m.name}</span><span className={m.answer === "Silent" || (typeof m.answer === "object" && "Failed" in m.answer) ? "m bad" : "m"}>{answerLabel(m.answer)}</span></div>)}</div>
        <div className="am-label">Files</div>
        <div className="rp-box">{draft.files.map(f => <button type="button" className="rp-row" key={f.path} aria-current={p.opened === f.path} aria-expanded={p.opened === f.path} onClick={() => p.openFile(f.path)}><code>{f.path}</code><span className="m">{sizeLabel(bytes.get(f.path) ?? f.text.length)}</span></button>)}</div>
        {shown && <><div className="rp-pane-head"><b><code>{shown.path}</code></b><small>Read only, secrets removed</small></div><pre className="rp-pane" aria-label={shown.path}>{shown.text.split(/(\[redacted\])/).map((part, i) => i % 2 ? <span className="r" key={i}>{part}</span> : part)}</pre></>}
        <div className="rp-total"><span>Total</span><span>{sizeLabel(total)}</span></div>
        {!draft.files.length && <div className="am-msg rp-none">No machine sent anything to report.</div>}
        {state.step === "sending" && <div className="am-wait rp-wait"><Spinner />Sending</div>}
        <div className="rp-foot"><Button look="quiet" disabled={state.step === "sending"} onClick={p.cancel}>Cancel</Button><Button look="primary" disabled={!canSend(state)} onClick={p.send}>Send</Button></div>
      </>;
    }
    case "sent":
      return <><Reference reference={state.reference ?? ""} />{state.gh ? <>
        <div className="am-label">Report it on GitHub too?</div>
        {p.issueText === null ? <div className="am-wait rp-wait"><Spinner />Writing the issue</div> : <pre className="rp-issue" aria-label="Issue text">{p.issueText}</pre>}
        <div className="rp-foot"><Button look="quiet" onClick={p.done}>No</Button><Button look="primary" disabled={p.issueText === null} onClick={e => p.openIssue(e.currentTarget)}>Yes</Button></div>
      </> : <div className="rp-foot"><Button look="primary" onClick={p.done}>Done</Button></div>}</>;
    case "issue":
      return <><Reference reference={state.reference ?? ""} /><div className="rp-row rp-link"><span className="m">Issue</span><code>{state.issueUrl ?? ""}</code></div><div className="rp-foot"><Button onClick={p.openUrl}>Open</Button><Button look="primary" onClick={p.done}>Done</Button></div></>;
    case "failed": {
      const hint = issueHint(state);
      return <><div className="rp-err" role="alert">{state.error ?? ""}</div>{hint && <div className="rp-hint">{hint}</div>}<div className="rp-foot"><Button look="quiet" onClick={p.back}>Back</Button><Button look="primary" onClick={e => p.retry(e.currentTarget)}>Try again</Button></div></>;
    }
  }
}

function Reference({ reference }: { reference: string }) {
  return <div className="rp-ref"><span>{reference}</span><CopyButton small text={reference} /></div>;
}
