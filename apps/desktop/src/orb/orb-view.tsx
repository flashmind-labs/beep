import { Fragment, useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import { OrbAsks } from "../chat/approvals.ts";
import { AppLogo } from "../chat/applogo-view.tsx";
import { invoke, Channel, listen } from "../ui/tauri.ts";
import { ZOOMED } from "../ui/zoom-state.ts";

interface Waiting { session: string; approval: { query_id: string; app: string; action: string; fields: { label: string; value: string }[] }; work: boolean }
interface Notice { id: number; title: string; snippet: string }
interface View { phase: string; words: string; line: string | null; speaking: boolean; title: string | null; finishing: boolean }
type LiveEvent = ({ kind: "view" } & View) | { kind: "level"; level: number } | { kind: "toast"; text: string } | { kind: "soon"; ms: number | null } | { kind: "sent" | "left" } | ({ kind: "approval" } & Waiting) | { kind: "answered" | "withdrawn"; query_id: string };
type Decision = "allow" | "always" | "deny" | "session";
type NoticeAction = "dismiss" | "play" | "open";
const ignore = () => {};
const clip = (s: string, n: number) => s.length > n ? `${s.slice(0, n - 1).trimEnd()}…` : s;

export function Orb() {
  const [view, setView] = useState<View | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [failure, setFailure] = useState("");
  const [modeOverride, setModeOverride] = useState<string | null>(null);
  const [life, resetLife] = useReducer((n: number) => n + 1, 0);
  const [, redrawAsks] = useReducer((n: number) => n + 1, 0);
  const asks = useRef(new OrbAsks<Waiting>());
  const currentNotice = useRef<Notice | null>(null);
  const live = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const frame = useRef<number | null>(null);
  const orb = useRef<HTMLDivElement>(null);
  const ring = useRef<SVGSVGElement>(null);
  const card = useRef<HTMLDivElement>(null);
  const note = useRef<HTMLDivElement>(null);
  const noticeEl = useRef<HTMLDivElement>(null);
  const waiting = asks.current.shown;
  const noteText = toast ?? (live.current && view?.finishing ? "Sending…" : live.current && view?.phase === "holding" ? "Let go to send" : "");
  const fit = useCallback(() => {
    if (frame.current !== null) cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      frame.current = null;
      const last = currentNotice.current ? noticeEl.current : asks.current.shown ? card.current : note.current?.hidden ? null : note.current;
      const height = last ? Math.ceil(last.getBoundingClientRect().bottom + parseFloat(getComputedStyle(last).marginBottom)) : 112;
      void invoke("fit_orb", { width: currentNotice.current || asks.current.shown ? 296 : last ? 240 : 112, height }).catch(ignore);
    });
  }, []);
  const soon = useCallback((ms: number | null) => {
    const el = ring.current;
    if (!el) return;
    el.style.display = "none";
    if (ms === null) return;
    el.style.setProperty("--ms", `${Math.max(1, ms)}ms`);
    void el.getBoundingClientRect();
    el.style.display = "";
  }, []);
  const ask = useCallback((w: Waiting) => {
    if (!asks.current.add(w)) return;
    if (asks.current.count === 1) setFailure("");
    redrawAsks();
  }, []);
  const answer = async (decision: Decision) => {
    const asked = asks.current.answer();
    if (!asked) return;
    setFailure(""); redrawAsks();
    const queryId = asked.approval.query_id;
    try {
      if (asked.work) await invoke("answer_work", { session: asked.session, queryId, decision });
      else await invoke("answer_approval", { session: asked.session, queryId, decision, onEvent: new Channel() });
    } catch (error) { asks.current.failed(asked); setFailure(String(error)); redrawAsks(); }
  };
  const noticeAction = (action: NoticeAction) => {
    const old = currentNotice.current;
    if (!old) return;
    currentNotice.current = null; setNotice(null);
    if (action === "play") setModeOverride("think");
    void invoke(`${action}_notice`, { id: old.id }).catch(ignore);
  };
  useLayoutEffect(() => { fit(); }, [fit, notice, waiting, noteText, failure, asks.current.count]);
  useEffect(() => {
    let closed = false;
    const stops: (() => void)[] = [];
    const register = (promise: Promise<() => void>) => { void promise.then(stop => { if (closed) stop(); else stops.push(stop); }).catch(ignore); };
    window.addEventListener(ZOOMED, fit);
    register(listen<Notice>("notice-card", ({ payload }) => { currentNotice.current = payload; setNotice(payload); if (!live.current) setModeOverride("idle"); }));
    register(listen<LiveEvent>("live", ({ payload: event }) => {
      if (event.kind === "view") { live.current = true; setView(event); setModeOverride(null); if (event.phase !== "listening") soon(null); }
      else if (event.kind === "level") orb.current?.style.setProperty("--lvl", Math.min(1, event.level).toFixed(3));
      else if (event.kind === "approval") ask({ session: event.session, approval: event.approval, work: event.work });
      else if (event.kind === "answered" || event.kind === "withdrawn") {
        const first = asks.current.shown?.approval.query_id === event.query_id;
        if (asks.current.remove(event.query_id)) { if (first) setFailure(""); redrawAsks(); }
      } else if (event.kind === "toast") {
        setToast(event.text); if (timer.current !== null) clearTimeout(timer.current);
        timer.current = setTimeout(() => setToast(null), 1600);
      } else if (event.kind === "soon") soon(event.ms);
      else if (event.kind === "sent") soon(null);
      else if (event.kind === "left") {
        asks.current.clear(); redrawAsks(); setFailure(""); live.current = false;
        if (timer.current !== null) clearTimeout(timer.current);
        setToast(null); soon(null);
      }
    }));
    void invoke<Waiting[]>("live_approval").then(items => { if (!closed) for (const item of items ?? []) ask(item); }).catch(ignore);
    void invoke<Notice | null>("notice_card").then(item => { if (!closed && item && !currentNotice.current) { currentNotice.current = item; setNotice(item); if (!live.current) setModeOverride("idle"); } }).catch(ignore);
    return () => { closed = true; stops.forEach(stop => stop()); window.removeEventListener(ZOOMED, fit); if (timer.current !== null) clearTimeout(timer.current); if (frame.current !== null) cancelAnimationFrame(frame.current); };
  }, [ask, fit, soon]);
  const mode = modeOverride ?? (view?.speaking || (view?.phase === "speaking" && view.line) ? "speak" : view?.phase === "thinking" ? "think" : view?.words ? "" : "idle");
  const replying = !view?.title && (view?.phase === "thinking" || view?.phase === "speaking");
  const fields = waiting?.approval.fields.slice(0, 3) ?? [];
  const more = (waiting?.approval.fields.length ?? 0) - fields.length;
  const behind = asks.current.count - 1;
  const grant = /for a routine$/.test(waiting?.approval.action ?? "");
  return <>
    <div className="ospot"><div className={`omini${live.current && view?.phase === "holding" ? " hold" : ""}`} id="mini" role="button" aria-label="Open Beep" onClick={() => currentNotice.current && !live.current ? noticeAction("open") : void invoke("show_bar").catch(ignore)}>
      <div className={`dorb ${mode}`} id="orb" ref={orb} aria-hidden="true"><span className="ring b"/><span className="ring"/><span className="core"/></div>
      <svg className="oring" id="ring" ref={ring} viewBox="0 0 70 70" aria-hidden="true" style={{ display: "none" }}><circle cx="35" cy="35" r="33"/></svg>
      <button className="ox" id="x" aria-label={view?.title ? "Stop the voice note" : replying ? "Cancel the reply" : "Leave live mode"} onClick={e => { e.stopPropagation(); if (currentNotice.current && !live.current) noticeAction("dismiss"); else void invoke("cancel_live").catch(ignore); }}><CloseIcon/></button>
    </div></div>
    <div className={`otoast${toast !== null && /could not|not available|busy|failed/i.test(toast) ? " bad" : ""}`} id="toast" ref={note} role="status" hidden={!noteText}>{noteText}</div>
    <div className="dcard oask" id="ask" ref={card} role="button" aria-label="Open the chat to see the whole request" hidden={!waiting} onClick={() => { const item = asks.current.shown; if (item) void invoke("open_approval", { session: item.session }).catch(ignore); }}>
      {waiting && <><div className="head"><AppLogo key={waiting.approval.query_id} app={waiting.approval.app} onReady={fit} /><div className="tt"><b title={`${waiting.approval.action}?`}>{waiting.approval.action}?</b><small>{waiting.work ? "Background work asks first" : `${waiting.approval.app} asks first`}</small></div></div>
      {fields.length > 0 && <div className="kv">{fields.map((f, i) => <Fragment key={i}><span>{clip(f.label, 24)}</span><span title={f.value}>{clip(f.value.replace(/\s+/g, " "), 120)}</span></Fragment>)}</div>}
      {more > 0 && <div className="more">{more} more in the chat</div>}{behind > 0 && <div className="more">{behind} more waiting after this</div>}{failure && <div className="err">{failure}</div>}
      <div className="acts">{([["allow", "Allow", "dbtn pri"], ...(!grant ? [["always", "Always", "dbtn"]] as const : []), ["deny", "Deny", "dbtn warn"], ...(!grant ? [["session", "Allow for this session", "dbtn wide"]] as const : [])] as const).map(([decision, label, cls]) => <button key={decision} className={cls} onClick={e => { e.stopPropagation(); void answer(decision); }}>{label}</button>)}</div></>}
    </div>
    <div className="dcard onote" id="notice" ref={noticeEl} role="button" aria-label="Open the chat" hidden={!notice} onClick={() => noticeAction("open")} onMouseEnter={() => { if (currentNotice.current) void invoke("hold_notice", { id: currentNotice.current.id, held: true }).catch(ignore); }} onMouseLeave={() => { if (currentNotice.current) { void invoke("hold_notice", { id: currentNotice.current.id, held: false }).catch(ignore); resetLife(); } }}>
      {notice && <><div className="head"><div className="tt"><b title={notice.title}>{notice.title}</b><small>New message</small></div><button className="nx" aria-label="Dismiss" onClick={e => { e.stopPropagation(); noticeAction("dismiss"); }}><CloseIcon/></button></div>{notice.snippet && <div className="snip">{notice.snippet}</div>}<div className="acts"><button className="dbtn pri" onClick={e => { e.stopPropagation(); noticeAction("play"); }}><svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>Play</button><button className="dbtn" onClick={e => { e.stopPropagation(); noticeAction("open"); }}>Open<kbd>⌥ ⌥</kbd></button></div><div className="life"><i key={life}/></div></>}
    </div>
  </>;
}
function CloseIcon() { return <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>; }
