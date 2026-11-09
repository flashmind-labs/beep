import type { Ago } from "../ui/dom.ts";
import { plural } from "../ui/format.ts";
import { mergeBlock, openFindings, reviewFacts, reviewState, STATE_LABEL, staleText, type ReviewHead, type ReviewView } from "./review.ts";

export function ReviewHeading({ review, now, ago, diff, openPr, again }: { review: ReviewView; now: ReviewHead | null; ago: Ago; diff: (() => void) | null; openPr: (url: string) => void; again: () => void }) {
  const state = reviewState(review), running = state === "running", stale = staleText(review, now);
  return <><div className="rv-head"><h3>Review {review.branch} into {review.base}</h3><span className={`rv-pill ${stale ? "changes" : state}`}>{running && <span className="dspin" />}{stale ? "Stale" : STATE_LABEL[state]}</span></div>
    <div className="rv-facts">{reviewFacts(review, ago).map((fact, index) => <span key={index} className={`rv-fact${fact.bad ? " bad" : ""}`}>{fact.spin ? <span className="dspin" /> : fact.dot != null ? <span className={`ddot${fact.dot ? ` ${fact.dot}` : ""}`} /> : null}{fact.pre}{fact.bold && <b>{fact.bold}</b>}{fact.code && <code>{fact.code}</code>}{fact.text}</span>)}
      <span className="rv-tools">{diff && <button type="button" className="dbtn" onClick={diff}>View diff</button>}{review.pr && <button type="button" className="dbtn" onClick={() => { if (review.pr) openPr(review.pr.url); }}>PR #{review.pr.number}</button>}{!running && <button type="button" className="dbtn" title="Reviews the branch head again" onClick={again}>Run again</button>}</span></div></>;
}

export function ReviewActions({ review, now, busy, fix, post, merge }: { review: ReviewView; now: ReviewHead | null; busy: string; fix: () => void; post: () => void; merge: () => void }) {
  const running = reviewState(review) === "running", open = openFindings(review).length, why = mergeBlock(review, now), mergeOk = !running && why.startsWith("Merge always");
  return <div className="rv-bar"><button type="button" className={`dbtn${open && !running ? " pri" : ""}`} disabled={running || !open || !!busy} title={open ? `Starts a task on ${review.branch} with ${plural(open, "finding", "findings")}` : ""} onClick={fix}>{busy === "fix" ? "Starting…" : "Fix with worker"}</button>
    <button type="button" className="dbtn" disabled={running || !review.pr || !!busy} title={review.pr ? "The project agent posts it; its card asks you first" : "The branch has no pull request"} onClick={post}>{busy === "post" ? "Asking…" : "Post review…"}</button>
    <button type="button" className={`dbtn${mergeOk ? " pri" : ""}`} disabled={!mergeOk || !!busy} onClick={merge}>{busy === "merge" ? "Asking…" : "Merge…"}</button><small>{why}</small></div>;
}
