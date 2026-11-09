import { plural, since } from "../ui/format.ts";
import type { ThreadCardView } from "./threadcard.ts";

const TEXT_LIMIT = 600;
function Post({ author, at, body, all }: { author: string; at: number | null; body: string; all: boolean }) {
  const text = all || body.length <= TEXT_LIMIT ? body : `${body.slice(0, TEXT_LIMIT).trimEnd()}...`;
  return <div className="c"><b>{author}</b> <span className="who">{at ? since(at) : "opened it"}</span>{body.trim() && <div className="tx">{text}</div>}</div>;
}
export function ThreadCardContent({ view, label, where, error, all, expand }: { view: ThreadCardView | null | undefined; label: string; where: string; error: string; all: boolean; expand(): void }) {
  if (view === undefined) return <span className="who">{error || <><span className="dspin" />{` Reading ${label}…`}</>}</span>;
  if (!view) return null;
  const occurrences = new Map<string, number>();
  const comments = view.comments.map(comment => {
    const id = `${comment.author}\0${comment.at}`, occurrence = occurrences.get(id) ?? 0;
    occurrences.set(id, occurrence + 1);
    return { comment, key: `${id}\0${occurrence}` };
  });
  const shown = all ? comments : comments.slice(-3), hidden = comments.length - shown.length;
  const long = [view.body, ...shown.map(item => item.comment.body)].some(body => body.length > TEXT_LIMIT);
  return <><span className="who">{where} · {plural(view.earlier + view.comments.length, "comment", "comments")}{view.needs_reply ? " · waits on your reply" : ""}</span>
    <Post author={view.author} at={null} body={view.body} all={all} />{shown.map(({ comment, key }) => <Post key={key} {...comment} all={all} />)}
    {(hidden > 0 || (long && !all)) && <button type="button" className="dbtn quiet small" onClick={expand}>{hidden > 0 ? `Show ${plural(hidden, "earlier comment", "earlier comments")}` : "Show all"}</button>}
    {all && view.earlier > 0 && <span className="who">{plural(view.earlier, "older comment", "older comments")} on the host</span>}{error && <span className="who">{error}</span>}
  </>;
}
