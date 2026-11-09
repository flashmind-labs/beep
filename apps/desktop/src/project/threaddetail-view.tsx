import { useLayoutEffect, useRef } from "react";
import { ago } from "../ui/format.ts";
import { patchChildren, patchHtml } from "../ui/patch.ts";
import { forgeLabel, type ThreadDetailRow, type ThreadStub } from "./threaddetail.ts";

const CHECKS: Record<string, [string, string]> = { failed: ["blocked", "checks failing"], passed: ["done", "checks passing"], running: ["running", "checks running"], queued: ["running", "checks queued"], waiting: ["running", "checks waiting"] };
function Chip({ kind, text, spin = false }: { kind: string; text: string; spin?: boolean }) {
  return <span className={kind ? `chip ${kind}` : "chip"}>{spin && <span className="dspin" />}{text}</span>;
}
function Markdown({ html, open }: { html: string; open(url: string): void }) {
  const node = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (node.current) patchHtml(node.current, html); }, [html]);
  return <div ref={node} className="pj-md" onClick={event => { const target = event.target; if (!(target instanceof Element)) return; const link = target.closest<HTMLAnchorElement>("a"); if (link) { event.preventDefault(); if (link.href) open(link.href); } }} />;
}
export function ThreadDetailView({ stub, data, error, acts, comment, text, posting, draftError, edit, post, open }: { stub: ThreadStub; data: ThreadDetailRow | null | undefined; error: string | undefined; acts: HTMLElement; comment: boolean; text: string; posting: boolean; draftError: string | undefined; edit(value: string): void; post(): void; open(url: string): void }) {
  const actions = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (actions.current) patchChildren(actions.current, [...acts.children]); }, [acts]);
  const author = data?.author || stub.author, opened = data?.opened_at || stub.opened_at, url = data?.url || stub.url, checks = data?.checks ? CHECKS[data.checks] : null;
  return <><div className="pj-dh" style={{ flex: "none" }}><h3>{data?.title || stub.title}</h3><div className="meta">{data && <Chip kind={data.state === "merged" ? "done" : data.state === "closed" ? "" : "running"} text={data.state === "open" && data.draft ? "draft" : data.state} />}<span>#{stub.number}{author && <> opened by <b>{author}</b></>}{opened ? ` ${ago(opened)}` : ""}</span>{data?.labels.map(label => <i className="pj-tag" key={label}>{label}</i>)}{data?.review === "approved" && <Chip kind="done" text="approved" />}{data?.review === "changes_requested" && <Chip kind="blocked" text="changes requested" />}{checks && <Chip kind={checks[0]} text={checks[1]} spin={checks[0] === "running"} />}</div><div className="acts"><div ref={actions} style={{ display: "contents" }} />{url && <button type="button" className="dbtn" onClick={() => open(url)}>{forgeLabel(url)}</button>}</div></div>
    <div className="pj-db" style={{ flex: "none" }}>{error && <div className="cc-err">{error}</div>}{data === undefined && !error && <div className="cc-panel cc-empty"><span>Loading…</span></div>}{data === null && <div className="cc-panel cc-empty"><span>The server needs an update before it shows the description and comments.</span></div>}
      {data && <>{data.body_html.trim() ? <Markdown html={data.body_html} open={open} /> : <p className="dmuted">No description.</p>}{data.earlier > 0 && <small className="dmuted">{data.earlier} earlier {data.earlier === 1 ? "comment" : "comments"} on the host</small>}{data.comments.map((item, index) => <div className="pj-post" key={`${item.author}:${item.at}:${index}`}><div className="by"><b>{item.author || "someone"}</b> · {ago(item.at)}</div><Markdown html={item.html} open={open} /></div>)}{comment && data.state === "open" ? <div className="pj-reply"><textarea placeholder="Comment" value={text} disabled={posting} onChange={event => edit(event.currentTarget.value)} /><button type="button" className="dbtn pri" disabled={posting || !text.trim()} onClick={post}>{posting ? "Posting…" : "Comment"}</button>{draftError && <div className="cc-err">{draftError}</div>}</div> : <small className="dmuted">Read only here. Comment on {forgeLabel(url).replace(/^Open on /, "")}.</small>}</>}
    </div></>;
}
