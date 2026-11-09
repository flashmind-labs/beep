import { useLayoutEffect, useRef } from "react";
import { since } from "../ui/format.ts";
import { going, hostCounts, hostLabel, progressText, runChip, runKey, runsCount, runSpan, type RunDetail, type RunRow, type RunsRow } from "./runs.ts";

export interface RunsViewProps {
  data: RunsRow | null | undefined; error: string; only: string | null; open: string | null; mode: HTMLElement | null;
  refresh(): void; filter(state: string | null): void; toggle(run: RunRow): void; hover(run: RunRow): void; leave(): void; menu(event: MouseEvent, run: RunRow): void;
  detail(run: RunRow): { data: RunDetail | null; error: string }; fix(detail: RunDetail): void; openUrl(url: string): void;
}
const marks: Record<string, string> = {
  passed: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>',
  failed: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M7 7l10 10M17 7L7 17"/></svg>',
  cancelled: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M7 12h10"/></svg>',
};
function Mode({ node }: { node: HTMLElement | null }) {
  const ref = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => { if (ref.current) ref.current.replaceChildren(...(node ? [node] : [])); }, [node]);
  return <span ref={ref} style={{ display: "contents" }} />;
}
function Empty({ children, error = false }: { children: string; error?: boolean }) { return <div className="cc-panel cc-empty"><span className={error ? "cc-err" : undefined}>{children}</span></div>; }
export function RunsView(p: RunsViewProps) {
  const d = p.data, perHost = d ? hostCounts(d) : [], why = p.error || d?.error || "";
  const current = d?.runs.filter(r => going(r.state)) ?? [], finished = d?.runs.filter(r => !going(r.state)) ?? [];
  const states = [...new Set(finished.map(r => r.state))].sort(), only = p.only && states.includes(p.only) ? p.only : null;
  const panel = (rows: RunRow[]) => <div className="cc-panel pj-runbox">{rows.map(r => <Run key={runKey(r)} run={r} p={p} />)}</div>;
  return <><div className="cc-sec"><h4>Actions</h4>{perHost.length ? perHost.map(c => <small key={c.host} className={c.error ? "hosterr" : undefined}>{c.text}</small>) : !!d?.runs.length && <small>{runsCount(d.runs)}</small>}{d?.synced_at ? <small className="sw-age">synced {since(d.synced_at)}</small> : null}<Mode node={p.mode} /><button type="button" className="dbtn quiet small" onClick={p.refresh}>Refresh</button></div>
    {d === undefined ? <Empty>Reading the runs…</Empty> : d === null ? <Empty>The server needs an update before it shows runs.</Empty> : <>
      {d.rate_limited_until && d.rate_limited_until > Date.now() ? <div className="cc-note"><span className="ddot warn" /><span>The host's rate limit for your token is low, so live progress waits until {new Date(d.rate_limited_until).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}.</span></div> : null}
      {!d.runs.length ? <Empty error={!!why}>{why || "No workflow runs yet."}</Empty> : <>{why && <div className="cc-err">{why}</div>}<div className="cc-sec"><h4>Running</h4><small>{current.length}</small></div>{current.length ? panel(current) : <Empty>Nothing running.</Empty>}<div className="cc-sec"><h4>Finished</h4><small>{finished.length}</small></div>{states.length > 1 && <div className="pj-labels">{states.map(state => <button type="button" key={state} className={`dbtn pj-tag lab${state === only ? " on" : ""}`} onClick={() => p.filter(state === only ? null : state)}>{runChip(state).text} {finished.filter(r => r.state === state).length}</button>)}</div>}{finished.length ? panel(only ? finished.filter(r => r.state === only) : finished) : <Empty>No finished runs.</Empty>}</>}
    </>}
  </>;
}
function Run({ run: r, p }: { run: RunRow; p: RunsViewProps }) {
  const chip = runChip(r.state), opened = p.open === runKey(r), hosts = p.data?.hosts ?? [], host = hostLabel(r.host, hosts);
  const detail = opened ? p.detail(r) : null;
  return <div className={`pj-runrow ${chip.cls || "idle"}${opened ? " open" : ""}`}><div className="rr-head" role="button" tabIndex={0} aria-expanded={opened} onClick={() => p.toggle(r)} onKeyDown={e => { if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); p.toggle(r); } }} onMouseEnter={() => p.hover(r)} onMouseLeave={p.leave} onContextMenu={e => p.menu(e.nativeEvent, r)}><span className={`rr-st ${chip.cls || "idle"}`} title={chip.text}>{chip.spin ? <span className="dspin" /> : <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: marks[r.state] ?? "" }} />}</span><div className="rr-body"><div className="rr-t">{r.host && hosts.length > 1 && <span className="pj-host" title={`${host} Actions on ${r.host}`}>{host}</span>}<b>{r.workflow} #{r.number}</b><span>{r.title}</span></div><div className="rr-m">{r.branch && <code className="br">{r.branch}</code>}{r.event && <span className="ev">{r.event}</span>}{r.actor && <span className="ev">{r.actor}</span>}{going(r.state) && r.progress && <><span>{progressText(r.progress)}</span>{r.progress.jobs > 1 && <span>{r.progress.jobs_done} of {r.progress.jobs} jobs done</span>}</>}</div></div><div className="rr-side"><span className="rr-when">{r.finished_at ? since(r.finished_at) : chip.text}</span><span className="rr-took">{going(r.state) ? "" : runSpan(r)}</span></div><span className="rr-acts"><button type="button" className="dbtn quiet small" title={`Open on ${host}`} onClick={e => { e.stopPropagation(); p.openUrl(r.url); }}>Open</button></span><span className={`rr-chev${opened ? " on" : ""}`} /></div>{detail && <Detail detail={detail} p={p} />}</div>;
}
function Detail({ detail, p }: { detail: { data: RunDetail | null; error: string }; p: RunsViewProps }) {
  const d = detail.data;
  if (!d) return <span className="pj-run">{detail.error ? <span className="cc-err">{detail.error}</span> : <small className="dmuted">Reading the run…</small>}</span>;
  const error = [detail.error, d.error].filter(Boolean).join(" "), shown = d.jobs.find(j => j.state === "failed") || d.jobs.find(j => going(j.state)), job = d.jobs.find(j => j.state === "failed");
  return <span className="pj-run">{error && <span className="cc-err">{error}</span>}{!d.jobs.length && <small className="dmuted">The host gives no jobs for this run.</small>}{!!d.jobs.length && <span className="pj-jobs">{d.jobs.map((j, i) => <span key={i} style={{ display: "contents" }}><b className={j.state === "failed" ? "bad" : undefined}>{j.name}</b><small>{[runChip(j.state).text, runSpan(j)].filter(Boolean).join(", ")}</small></span>)}</span>}{!!shown?.steps.length && <span className="pj-steps">{shown.steps.map((step, i) => { const cls = step.state === "failed" ? "bad" : going(step.state) ? "run" : step.state === "skipped" || step.state === "cancelled" ? "skip" : "ok"; return <span className={`pj-step ${cls}`} key={i}><span className={step.state === "failed" ? "ddot bad" : going(step.state) ? "ddot" : "ddot off"} /><span>{i + 1}. {step.name}</span><small>{step.state === "skipped" ? "skipped" : runSpan(step)}</small></span>; })}</span>}{d.log && <span className="pj-log"><small>End of the log of {[d.log.job, d.log.step].filter(Boolean).join(", ")}{d.log.truncated ? " (earlier lines left out)" : ""}</small><pre>{d.log.text}</pre></span>}{(d.run.state === "failed" || job?.url) && <span className="acts">{d.run.state === "failed" && <button type="button" className="dbtn pri" title="Start a coding agent on this run's branch with the failure in its prompt" onClick={() => p.fix(d)}>Fix this</button>}{job?.url && <button type="button" className="dbtn" onClick={() => { if (job.url) p.openUrl(job.url); }}>Full log</button>}</span>}</span>;
}
