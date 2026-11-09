import { plural } from "../ui/format.ts";
import type { ReviewView } from "./review.ts";

export function ReviewStatus({ starting, review, error, note, stale, branches, again }: {
  starting: string | null; review: ReviewView | null; error: string; note: string; stale: string; branches: () => void; again: () => void;
}) {
  if (starting) return <div className="cc-panel cc-empty"><span className="dspin" /><b>Starting the review of {starting}</b></div>;
  if (!review) return <>{error && <ErrorNote text={error} />}<div className="cc-panel cc-empty"><b>No review open</b><button type="button" className="dbtn" onClick={branches}>Branches</button></div></>;
  const conflicts = review.merge === "conflicts", failing = review.pr?.checks === "failing";
  return <>{error && <ErrorNote text={error} />}{note && <div className="cc-note"><span className="ddot" /><span>{note}</span></div>}
    {stale && <div className="cc-note"><span className="ddot warn" /><span><b>{stale}</b> The findings are for {review.head.slice(0, 7)}; Merge is off until the new head is reviewed.</span><button type="button" className="dbtn pri" onClick={again}>Review again</button></div>}
    {(conflicts || failing) && <div className="cc-note rv-cfl"><span className="ddot bad" /><span className="rv-cfl-t">{conflicts && <><b>Conflicts with {review.base} in {plural(review.conflicts.length, "file", "files")}</b><span>{review.conflicts.slice(0, 6).map((file, index) => <span key={`${index}:${file}`}>{index > 0 && ", "}<code>{file}</code></span>)}{review.conflicts.length > 6 && ` and ${review.conflicts.length - 6} more`}</span></>}{failing && <><b>CI failing</b><span>Checks fail on PR #{review.pr?.number}. The review still ran.</span></>}</span></div>}</>;
}

function ErrorNote({ text }: { text: string }) { return <div className="cc-note"><span className="ddot bad" /><span className="cc-err">{text}</span></div>; }
