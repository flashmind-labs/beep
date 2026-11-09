import type { ConnectProblem } from "./connect.ts";

export function ConnectingView({ problem, stuck, text, loading, open, retry, log }: { problem: ConnectProblem | null; stuck: boolean; text: string; loading: boolean; open(): void; retry(): void; log(): void }) {
  if (problem) return <div className="cc-conn stuck" data-problem={`${problem.kind}:${problem.text}`}><span className={`ddot ${problem.kind === "unlinked" || problem.kind === "noserver" ? "off" : "bad"}`} /><b>{problem.title}</b><span className="why">{problem.text}</span><div className="acts">{problem.action && <button type="button" className="dbtn pri" onClick={open}>{problem.action}</button>}<button type="button" className={`dbtn retry${problem.action ? "" : " pri"}`} disabled={loading} onClick={retry}>Try again</button><button type="button" className="dbtn" onClick={log}>Show log</button></div></div>;
  const pulse = <div className="cc-pulse"><i /><i /><i /></div>;
  if (!stuck) return <div className="cc-conn">{pulse}<b>Connecting to Beep…</b></div>;
  return <div className="cc-conn stuck">{pulse}<b>Still connecting to Beep</b><span className="why">{text}</span><div className="acts"><button type="button" className="dbtn pri" disabled={loading} onClick={retry}>Retry now</button><button type="button" className="dbtn" onClick={log}>Show log</button></div></div>;
}
