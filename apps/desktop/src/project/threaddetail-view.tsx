import { useSyncExternalStore, type ReactElement } from "react";
import { formatAgo } from "../ui/format.ts";
import { forgeLabel, type ThreadDetailRow, type ThreadStub } from "./threaddetail.tsx";
import { Button, Chip, EmptyPanel, useHtml } from "../ui/kit.tsx";
import { FeedbackControl, type FeedbackState } from "../review/feedback-view.tsx";

const CHECKS: Record<string, [string, string]> = { failed: ["blocked", "checks failing"], passed: ["done", "checks passing"], running: ["running", "checks running"], queued: ["running", "checks queued"], waiting: ["running", "checks waiting"] };
function Markdown({ html, open }: { html: string; open(url: string): void }) {
  const body = useHtml(html);

  return <div className="pj-md" onClick={event => { const target = event.target; if (!(target instanceof Element)) return; const link = target.closest<HTMLAnchorElement>("a"); if (link) { event.preventDefault(); if (link.href) open(link.href); } }}>{body}</div>;
}
export interface ThreadActions { buttons: ThreadAction[]; error: string | null }

/// A thread's button: a plain one, or the agent button (Review with agent,
/// Work on it) whose picker starts a session.
export type ThreadAction =
  | { kind: "run"; key: string; label: string; primary: boolean; disabled: boolean; title: string; run(): void }
  | { kind: "agent"; key: string; button: ThreadButton; state: AgentState };

/// The agent button as its thread has it; the picker adds whether it is open.
export type AgentState =
  | { kind: "ready"; label: string; hint: string }
  | { kind: "starting" }
  | { kind: "running"; agent: string; hint: string; open(): void };

/// The agent button's picker, kept per thread across draws.
export interface ThreadButton {
  snapshot: () => { open: boolean; picker: ReactElement | null };
  subscribe: (listener: () => void) => () => void;
  setBox: (box: HTMLElement | null) => void;
  setButton: (button: HTMLButtonElement | null) => void;
  setSlot: (slot: HTMLDivElement | null) => void;
  click: () => void;
  /// Why it cannot start, or "".
  why: () => string;
}

function AgentButton({ button, state }: { button: ThreadButton; state: AgentState }) {
  const { open, picker } = useSyncExternalStore(button.subscribe, button.snapshot);
  const shown: FeedbackState = state.kind === "ready" ? { ...state, reason: button.why(), open }
    : state.kind === "running" ? { kind: "running", agent: state.agent, hint: state.hint } : state;
  return <span className="dv-fbw" ref={button.setBox}>
    <FeedbackControl state={shown} setButton={button.setButton} setSlot={button.setSlot} onClick={state.kind === "running" ? state.open : button.click} picker={picker} />
  </span>;
}

function ActionButton({ action }: { action: ThreadAction }) {
  switch (action.kind) {
    case "run": return <Button look={action.primary ? "primary" : "secondary"} disabled={action.disabled} title={action.title} onClick={action.run}>{action.label}</Button>;
    case "agent": return <AgentButton button={action.button} state={action.state} />;
  }
}
export function ThreadDetailView({ stub, data, error, acts, text, posting, draftError, edit, post, open }: { stub: ThreadStub; data: ThreadDetailRow | null | undefined; error: string | undefined; acts: ThreadActions; text: string; posting: boolean; draftError: string | undefined; edit(value: string): void; post(): void; open(url: string): void }) {
  const author = data?.author || stub.author, opened = data?.opened_at || stub.opened_at, url = data?.url || stub.url, checks = data?.checks ? CHECKS[data.checks] : null;
  return <><div className="pj-dh" style={{ flex: "none" }}><h3>{data?.title || stub.title}</h3><div className="meta">{data && <Chip kind={data.state === "merged" ? "done" : data.state === "closed" ? "" : "running"} text={data.state === "open" && data.draft ? "draft" : data.state} />}<span>#{stub.number}{author && <> opened by <b>{author}</b></>}{opened ? ` ${formatAgo(opened, "short")}` : ""}</span>{data?.labels.map(label => <i className="pj-tag" key={label}>{label}</i>)}{data?.review === "approved" && <Chip kind="done" text="approved" />}{data?.review === "changes_requested" && <Chip kind="blocked" text="changes requested" />}{checks && <Chip kind={checks[0]} text={checks[1]} spin={checks[0] === "running"} />}</div><div className="acts"><span className="acts">{acts.buttons.map(action => <ActionButton key={action.key} action={action} />)}{acts.error && <span className="cc-err">{acts.error}</span>}</span>{url && <Button onClick={() => open(url)}>{forgeLabel(url)}</Button>}</div></div>
    <div className="pj-db" style={{ flex: "none" }}>{error && <div className="cc-err">{error}</div>}{data === undefined && !error && <EmptyPanel text="Loading…" />}{data === null && <EmptyPanel text="No machine with the project could read the description and comments." />}
      {data && <>{data.body_html.trim() ? <Markdown html={data.body_html} open={open} /> : <p className="dmuted">No description.</p>}{data.earlier > 0 && <small className="dmuted">{data.earlier} earlier {data.earlier === 1 ? "comment" : "comments"} on the host</small>}{data.comments.map((item, index) => <div className="pj-post" key={`${item.author}:${item.at}:${index}`}><div className="by"><b>{item.author || "someone"}</b> · {formatAgo(item.at, "short")}</div><Markdown html={item.html} open={open} /></div>)}{data.state === "open" ? <div className="pj-reply"><textarea placeholder="Comment" value={text} disabled={posting} onChange={event => edit(event.currentTarget.value)} /><Button look="primary" disabled={posting || !text.trim()} onClick={post}>{posting ? "Posting…" : "Comment"}</Button>{draftError && <div className="cc-err">{draftError}</div>}</div> : <small className="dmuted">Read only here. Comment on {forgeLabel(url).replace(/^Open on /, "")}.</small>}</>}
    </div></>;
}
