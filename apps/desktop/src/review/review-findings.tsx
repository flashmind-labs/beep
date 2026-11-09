import { harnessInner, harnessMark } from "../ui/harnessicon.ts";
import { findingsSub, isRunning, openFindings, readySummary, shardChip, shardLine, type FindingView, type ReviewView } from "./review.ts";

export function ReviewFindings({ review, busy, dismiss }: { review: ReviewView; busy: string; dismiss: (finding: FindingView) => void }) {
  const running = isRunning(review), findings = review.findings || [];
  return <><div><div className="cc-sec"><h4>Findings</h4><small>{findingsSub(review)}</small></div>
    {findings.length ? <div className="cc-panel">{findings.map(f => <Finding key={f.id} finding={f} running={running} busy={busy === `f${f.id}`} dismiss={() => dismiss(f)} />)}
      {running && <div className="cc-empty rv-more">More findings show as the reviewers finish.</div>}
      {!running && !openFindings(review).length && <div className="rv-sum-row">{readySummary(review)}</div>}</div>
      : running ? <div className="cc-panel cc-empty"><span className="dspin" /><span>Findings show as the reviewers finish.</span></div>
      : <div className="cc-panel cc-empty"><b>No findings</b><span className="rv-sum">{readySummary(review)}</span></div>}</div>
    <div><div className="cc-sec"><h4>Shards</h4></div><div className="cc-panel">{review.shards.map((shard, index) => {
      const chip = shardChip(shard), safe = /^[a-z0-9-]+$/.test(shard.agent) ? ` ${shard.agent}` : "";
      return <div className="rv-shard" key={`${shard.area}:${shard.agent}:${index}`}><span className={`cc-ag${safe}${harnessMark(shard.agent) ? " mark" : ""}`} dangerouslySetInnerHTML={{ __html: harnessInner(shard.agent) }} />
        <span className="t"><b>{shard.area}</b><span>{shardLine(shard, review.machine_name)}</span></span><span className={`chip ${chip.cls}`} title={shard.note || chip.text}>{chip.spin && <span className="dspin" />}{chip.text}</span></div>;
    })}</div></div></>;
}

function Finding({ finding: f, running, busy, dismiss }: { finding: FindingView; running: boolean; busy: boolean; dismiss: () => void }) {
  const dropped = f.verdict === "dropped";
  return <div className={`rv-find${dropped || f.dismissed ? " gone" : ""}`}><span className={`chip ${dropped ? "" : f.severity}`}>{dropped ? "dropped" : f.dismissed ? "dismissed" : f.severity}</span>
    <span className="t"><b>{f.title}</b>{!dropped && <code>{f.line ? `${f.file}:${f.line}` : f.file}</code>}<span>{dropped ? `Not real: ${f.dropped_why || "the verifier found no problem"}` : f.why}</span></span>
    <span className="rv-side">{dropped ? <span /> : f.verdict === "waiting" ? <span className="rv-ver wait"><span className="dspin" />verifying</span> : f.verdict === "verified" ? <span className="rv-ver"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round"><path d="M5 12l5 5 9-10" /></svg>Verified</span> : <span className="rv-ver wait">Not verified</span>}
      {!dropped && !running && <button type="button" className="dbtn quiet small" disabled={busy} onClick={dismiss}>{f.dismissed ? "Bring back" : "Dismiss"}</button>}</span></div>;
}
