import { openUrl } from "../center/openitems.ts";
import { formatAgo } from "../ui/format.ts";
import type { LinkedPrRow, ThreadDetailRow, ThreadStub } from "./threaddetail.tsx";
import { Button, Chip, EmptyPanel, LineRow, Section, useHtml } from "../ui/kit.tsx";
import { AgentButton, type AgentState, type PickButton } from "../review/feedback-view.tsx";

/// "Open on GitHub", else "Open on Forgejo": a project's host is one or the other.
function forgeLabel(url: string) {
  try { return new URL(url).host === "github.com" ? "Open on GitHub" : "Open on Forgejo"; } catch { return "Open on GitHub"; }
}

/// The chip kind of a thread's state.
const STATE_CHIP: Record<ThreadDetailRow["state"], string> = { open: "running", closed: "", merged: "done" };
const CHECKS: Record<string, [string, string]> = { failed: ["blocked", "checks failing"], passed: ["done", "checks passing"], running: ["running", "checks running"], queued: ["running", "checks queued"], waiting: ["running", "checks waiting"] };
function Markdown({ html, open }: { html: string; open(url: string): void }) {
  const body = useHtml(html);

  return <div className="pj-md" onClick={event => { const target = event.target; if (!(target instanceof Element)) return; const link = target.closest<HTMLAnchorElement>("a"); if (link) { event.preventDefault(); if (link.href) open(link.href); } }}>{body}</div>;
}
export interface ThreadActions { buttons: ThreadAction[]; error: string | null }

/// A thread's button: a plain one, or the agent button (Continue, Review
/// with agent, Work on it) whose picker starts a session.
export type ThreadAction =
  | { kind: "run"; key: string; label: string; primary: boolean; disabled: boolean; title: string; run(): void }
  | { kind: "agent"; key: string; button: PickButton; state: AgentState };

function ActionButton({ action }: { action: ThreadAction }) {
  switch (action.kind) {
    case "run": return <Button look={action.primary ? "primary" : "secondary"} disabled={action.disabled} title={action.title} onClick={action.run}>{action.label}</Button>;
    case "agent": return <AgentButton item={{ button: action.button, state: action.state, error: "" }} />;
  }
}
export function ThreadDetailView({ stub, data, error, acts, text, posting, draftError, edit, post, open }: { stub: ThreadStub; data: ThreadDetailRow | null | undefined; error: string | undefined; acts: ThreadActions; text: string; posting: boolean; draftError: string | undefined; edit(value: string): void; post(): void; open(url: string): void }) {
  const author = data?.author || stub.author, opened = data?.opened_at || stub.opened_at, url = data?.url || stub.url, checks = data?.checks ? CHECKS[data.checks] : null;
  return <><div className="pj-dh" style={{ flex: "none" }}><h3>{data?.title || stub.title}</h3><div className="meta">{data && <Chip kind={STATE_CHIP[data.state]} text={data.state === "open" && data.draft ? "draft" : data.state} />}<span>#{stub.number}{author && <> opened by <b>{author}</b></>}{opened ? ` ${formatAgo(opened, "short")}` : ""}</span>{data?.labels.map(label => <i className="pj-tag" key={label}>{label}</i>)}{data?.review === "approved" && <Chip kind="done" text="approved" />}{data?.review === "changes_requested" && <Chip kind="blocked" text="changes requested" />}{checks && <Chip kind={checks[0]} text={checks[1]} spin={checks[0] === "running"} />}</div><div className="acts"><span className="acts">{acts.buttons.map(action => <ActionButton key={action.key} action={action} />)}{acts.error && <span className="cc-err">{acts.error}</span>}</span>{url && <Button onClick={() => openUrl(url)}>{forgeLabel(url)}</Button>}</div></div>
    <div className="pj-db" style={{ flex: "none" }}>{error && <div className="cc-err">{error}</div>}{data === undefined && !error && <EmptyPanel text="Loading…" />}{data === null && <EmptyPanel text="No machine with the project could read the description and comments." />}
      {data && <>{data.linked_prs.length > 0 && <LinkedPrs prs={data.linked_prs} open={open} />}{data.body_html.trim() ? <Markdown html={data.body_html} open={open} /> : <p className="dmuted">No description.</p>}{data.earlier > 0 && <small className="dmuted">{data.earlier} earlier {data.earlier === 1 ? "comment" : "comments"} on the host</small>}{data.comments.map((item, index) => <div className="pj-post" key={`${item.author}:${item.at}:${index}`}><div className="by"><b>{item.author || "someone"}</b> · {formatAgo(item.at, "short")}</div><Markdown html={item.html} open={open} /></div>)}{data.state === "open" ? <div className="pj-reply"><textarea autoComplete="off" autoCorrect="off" placeholder="Comment" value={text} disabled={posting} onChange={event => edit(event.currentTarget.value)} /><Button look="primary" disabled={posting || !text.trim()} onClick={post}>{posting ? "Posting…" : "Comment"}</Button>{draftError && <div className="cc-err">{draftError}</div>}</div> : <small className="dmuted">Read only here. Comment on {forgeLabel(url).replace(/^Open on /, "")}.</small>}</>}
    </div></>;
}

/// The pull requests linked to an issue, each opening that pull request.
function LinkedPrs({ prs, open }: { prs: LinkedPrRow[]; open(url: string): void }) {
  return <Section title="Pull requests" count={prs.length}>{prs.map(pr => <LineRow key={pr.url} lead={<span className="num">{`#${pr.number}`}</span>} title={pr.title} sub={repoOf(pr.url)} right={<Chip kind={STATE_CHIP[pr.state]} text={pr.state} />} onOpen={() => open(pr.url)} />)}</Section>;
}

/// `owner/repo` of a pull request's web page.
function repoOf(url: string) {
  try { return new URL(url).pathname.split("/").slice(1, 3).join("/"); } catch { return ""; }
}
