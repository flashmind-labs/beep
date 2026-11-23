// A coding agent session opened as a split tab inside `PaneDock`, beside or
// under the primary conversation: polls `agent_view` while shown, renders its
// entries and open question, and lets the user steer, answer or promote it.
import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import type { AgentEntry, AgentPage, AgentQuestion, AgentRow } from "../agents/agents.ts";
import { parseTool } from "../agents/agentlog.ts";
import { AGENT_ACTIVE } from "../agents/sessionmenu-state.ts";
import { agentName } from "../agents/running-state.ts";
import { promptParts } from "../agents/promptparts-state.ts";
import { AgentStatus } from "../agents/mode-view.tsx";
import { base, linkified, reason } from "../ui/format.ts";
import { HarnessIcon, Icon } from "../ui/icons.tsx";
import { Button, EmptyPanel, Spinner, useHtml } from "../ui/kit.tsx";
import { conflicted, invoke } from "../ui/tauri.ts";
import type { SplitSession } from "./media.ts";

const ACTIVE_POLL_MS = 1500;
const IDLE_POLL_MS = 5000;

export function SessionTabView({ session, hidden, close }: { session: SplitSession; hidden: boolean; close(): void }) {
  const [page, setPage] = useState<AgentPage | null>(null);
  const [error, setError] = useState("");
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [sendErr, setSendErr] = useState("");
  const [answering, setAnswering] = useState<number | null>(null);
  const [tick, setTick] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);

  useEffect(() => {
    if (hidden) return;
    let cancelled = false;
    let timer = 0;

    const poll = async () => {
      try {
        const next = await invoke<AgentPage>("agent_view", { machineId: session.machine_id, agent: session.agent, session: session.id });
        if (cancelled) return;
        setPage(next);
        setError("");
        const ms = AGENT_ACTIVE.has(next.session.status) ? ACTIVE_POLL_MS : IDLE_POLL_MS;
        if (!document.hidden) timer = window.setTimeout(() => { void poll(); }, ms);
      } catch (e) {
        if (cancelled) return;
        setError(reason(e));
        if (!document.hidden) timer = window.setTimeout(() => { void poll(); }, IDLE_POLL_MS);
      }
    };

    const onVis = () => { if (!document.hidden && !timer) void poll(); };
    document.addEventListener("visibilitychange", onVis);
    void poll();
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, [session.machine_id, session.agent, session.id, hidden, tick]);

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
    try {
      const steer = active ? { steer: { text } } : { prompt: { text } };
      await invoke("agent_steer", { machineId: session.machine_id, agent: session.agent, session: session.id, steer, seen: null });
      setDraft("");
      stuck.current = true;
      setTick(n => n + 1);
    } catch (e) {
      if (!conflicted(e)) setSendErr(reason(e));
      else { setDraft(""); setTick(n => n + 1); }
    } finally {
      setSending(false);
    }
  };

  const stop = () => {
    void invoke("agent_steer", { machineId: session.machine_id, agent: session.agent, session: session.id, steer: "cancel", seen: null })
      .then(() => setTick(n => n + 1))
      .catch(err => setSendErr(reason(err)));
  };

  const answer = async (q: AgentQuestion, reply: string) => {
    setAnswering(q.id);
    try {
      await invoke("center_answer_agent", { machineId: session.machine_id, agent: session.agent, session: session.id, question: q.id, reply });
      setTick(n => n + 1);
    } catch (e) {
      if (!conflicted(e)) setSendErr(reason(e));
    } finally {
      setAnswering(null);
    }
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
      <textarea rows={1} placeholder={`Message ${agentName(session.agent)}`} aria-label={`Message ${current?.title || agentName(session.agent)}`}
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
