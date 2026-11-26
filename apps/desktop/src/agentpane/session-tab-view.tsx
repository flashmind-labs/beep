// A coding agent session opened as a split tab inside `PaneDock`, beside or
// under the primary conversation: draws what the app holds of it at once, follows
// its pushed updates while shown, renders its entries and open question, and lets
// the user steer, answer or promote it.
import { useEffect, useRef, useState, useSyncExternalStore, type FormEvent, type KeyboardEvent } from "react";
import type { AgentEntry, AgentPage, AgentQuestion, AgentRow } from "../agents/agents-state.ts";
import { openOf, where } from "../agents/conversation-state.ts";
import { AGENT_ACTIVE } from "../agents/sessionmenu-state.ts";
import { parseTool } from "../agents/agentlog.ts";
import type { replyToQuestion, sendOnce } from "../agents/messages.ts";
import type { feed, keptTranscript } from "../agents/session-follow.ts";
import { transcriptKey, transcripts } from "../agents/transcript-cache-state.ts";
import { agentName } from "../agents/running-state.ts";
import { promptParts } from "../agents/promptparts-state.ts";
import { AgentStatus } from "../agents/mode-view.tsx";
import { base, linkified, reason } from "../ui/format.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { Button, EmptyPanel, NO_AUTO, Spinner, useHtml } from "../ui/kit.tsx";
import { conflicted, invoke } from "../ui/tauri.ts";
import type { SplitSession } from "./pane-state.ts";

/// What the tab calls on the session modules, handed in by the dock so this view does not import
/// the modules that reach back to it.

export interface SessionTabLinks {
  feed: Pick<typeof feed, "hold" | "live">;
  keptTranscript: typeof keptTranscript;
  sendOnce: typeof sendOnce;
  replyToQuestion: typeof replyToQuestion;
}

export function SessionTabView({ session, hidden, close, links }: { session: SplitSession; hidden: boolean; close(): void; links: SessionTabLinks }) {
  const { feed, keptTranscript, sendOnce, replyToQuestion } = links;
  const key = transcriptKey(openOf(session));
  const held = useSyncExternalStore(transcripts.subscribe, () => transcripts.peek(key));
  const [error, setError] = useState("");
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [sendErr, setSendErr] = useState("");
  const [answering, setAnswering] = useState<number | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);
  const page = held?.page ?? null;

  useEffect(() => {
    if (hidden) return;
    const o = openOf(session);
    let shown = true;
    let release = () => {};
    // A page read by other means, the one kept on disk or the one read when the follow could not
    // open, is held like a pushed one, as a snapshot with no cursor the node could resume from.
    const keep = (page: AgentPage) => {
      if (!shown) return;
      const next = page.earlier + page.entries.length;
      transcripts.receive(key, { kind: "snapshot", page, cursor: { start: page.earlier, next, fingerprint: "" }, resumed: false });
      setError("");
    };

    // A page read after the follow went live is older than what it pushed.
    const read = () => invoke<AgentPage>("agent_view", where(o)).then(page => { if (!links.feed.live(o)) keep(page); }, e => { if (shown) setError(reason(e)); });

    // A follow that cannot open is read once; the feed opens it again every few seconds.
    const hold = () => {
      if (!shown) return;
      release = feed.hold(o, () => void read());
    };

    // The page kept on disk draws at once, and the follow goes on from its cursor.
    if (transcripts.peek(key)) hold();
    else void keptTranscript(o).then(kept => { if (kept && !transcripts.peek(key)) keep(kept); hold(); });

    return () => { shown = false; release(); };
  }, [key, hidden]);

  useEffect(() => {
    const el = scroller.current;
    if (el && stuck.current) el.scrollTop = el.scrollHeight;
  }, [page?.entries.length, page?.session.question?.id]);

  const current = page?.session ?? null;
  const status = current?.status ?? session.status;
  const active = AGENT_ACTIVE.has(status);
  const writable = !(current?.external ?? session.external);
  const question = current?.question ?? null;

  const promote = () => {
    const row: AgentRow = current ?? {
      machine_id: session.machine_id, machine_name: session.machine_name, id: session.id, agent: session.agent,
      title: session.title, cwd: session.cwd, status: session.status, doing: null, started_at: 0, last_activity: 0,
      external: session.external, question: null, assistant: false, persona: null,
    };
    close();
    void import("../agents/session.tsx").then(m => m.openAgent(row));
  };

  const submit = async (event?: FormEvent) => {
    event?.preventDefault();
    const text = draft.trim();
    if (!text || sending || !writable) return;
    setSending(true);
    setSendErr("");
    const failed = await sendOnce(openOf(session), active ? { steer: { text } } : { prompt: { text } }, null);

    // A conflict means the session moved on: the words are dropped, as when sent.
    if (!failed || conflicted(failed.message)) {
      setDraft("");
      stuck.current = true;
    } else {
      setSendErr(failed.message);
    }

    setSending(false);
  };

  const stop = () => {
    void sendOnce(openOf(session), { cancel: null }, null).then(failed => { if (failed) setSendErr(failed.message); });
  };

  const answer = async (q: AgentQuestion, reply: string) => {
    setAnswering(q.id);
    const failed = await replyToQuestion(openOf(session), q.id, reply);
    if (failed && !conflicted(failed.error)) setSendErr(reason(failed.error));
    setAnswering(null);
  };

  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void submit();
    }
  };

  return <div className="agsplit">
    <div className="agsplit-head">
      <HarnessIcon agent={session.agent} className="dapp dcoderav" />
      <span className="tt">
        <b>{current?.title || session.title || agentName(session.agent)}</b>
        <small>{[base(current?.cwd || session.cwd), current?.machine_name || session.machine_name].filter(Boolean).join(" @ ")}</small>
      </span>
      <AgentStatus status={status} stopping={false} />
      <Button look="quiet" small onClick={promote} title="Move this session to the main pane">Open main</Button>
    </div>
    <div className="dbody agsplit-body" ref={scroller} onScroll={event => {
      const el = event.currentTarget;
      stuck.current = el.scrollHeight - el.scrollTop - el.clientHeight < 32;
    }}>
      {!page && !error && <div className="dmuted"><Spinner /> Loading session…</div>}
      {error && <div className="derr">{error}</div>}
      {page && !page.entries.length && !question && <EmptyPanel text="No messages yet." />}
      {page && <div className="dagmain">{page.entries.map((entry, index) => <SplitEntry key={index} entry={entry} />)}</div>}
      {question && <SplitQuestion question={question} busy={answering === question.id} answer={reply => { void answer(question, reply); }} />}
      {page?.error && <div className="derr">{page.error}</div>}
    </div>
    {sendErr && <div className="derr agsplit-err">{sendErr}</div>}
    {writable && <form className="dinput small agsplit-field" onSubmit={event => { void submit(event); }}>
      <textarea rows={1} {...NO_AUTO} placeholder={`Message ${agentName(session.agent)}`} aria-label={`Message ${current?.title || agentName(session.agent)}`}
        value={draft} disabled={sending} onChange={event => setDraft(event.currentTarget.value)} onKeyDown={onKey} />
      {active && <Button small type="button" onClick={stop}>Stop</Button>}
      <Button look="primary" small type="submit" aria-label="Send" disabled={sending || !draft.trim()}><Icon name="send" /></Button>
    </form>}
  </div>;
}

function SplitEntry({ entry }: { entry: AgentEntry }) {
  if (entry.kind === "user") {
    const userText = promptParts(entry.text).filter(part => part.kind === "user").map(part => part.text).join("\n").trim() || entry.text;
    return <div className="dcoderen user">{userText}</div>;
  }
  if (entry.kind === "tool") {
    const { title, status } = parseTool(entry.text);
    const line = title.split("\n")[0];
    return <div className={`agsplit-tool${status === "failed" ? " bad" : ""}`}><span>{line}</span>{status && <small>{status === "in_progress" ? "running" : status}</small>}</div>;
  }
  if (entry.kind === "thought") return null;
  if (entry.kind === "other") return <div className="dmuted">{entry.text}</div>;
  return <SplitAgentReply entry={entry} />;
}

function SplitAgentReply({ entry }: { entry: AgentEntry & { html?: string } }) {
  const html = useHtml(entry.html || linkified(entry.text));
  return <div className="dcoderen agent md">{html}</div>;
}

function SplitQuestion({ question, busy, answer }: { question: AgentQuestion; busy: boolean; answer(reply: string): void }) {
  const allow = question.options.findIndex(opt => opt.allow);
  return <div className="dcard dcoder">
    <div className="q">
      <b>{question.text}</b>
      {question.detail && <code>{question.detail}</code>}
      <div className="dups">
        {question.options.map((option, index) => <Button key={option.id} look={index === allow ? "primary" : "secondary"} danger={!option.allow && allow >= 0}
          disabled={busy} onClick={() => answer(option.id)}>{option.name}</Button>)}
      </div>
    </div>
  </div>;
}
