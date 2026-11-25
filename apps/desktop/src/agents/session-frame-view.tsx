import { memo, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";
import { useStore, type StoreApi } from "zustand";
import { HintView } from "../bar/hints-view.tsx";
import { StashActions, StashIndicator } from "../chat/stash-hints.tsx";
import { SHORTCUTS } from "../ui/shortcuts.ts";
import type { ChatBody } from "../ui/turn.ts";
import { conversationAppearance } from "../ui/conversation-appearance.ts";
import { PaneDock, PaneDropZone, PaneMain, PaneShell } from "../agentpane/dock-view.tsx";
import { onLogDrawn, promptMarks, revealLogUnit } from "./logwindow.tsx";
import { syncJump } from "./logview.tsx";
import type { AgentHintsModel, Frame, FrameState, Slot } from "./session-frame.tsx";

const SlotView = memo(function SlotView({ slot }: { slot: Slot }) {
  const view = useStore(slot.view);
  return slot.host ? createPortal(view, slot.host) : view;
});

/// The bar's session page, drawn from its frame's store.
export function SessionFrame({ store }: { store: StoreApi<FrameState> }) {
  const { frame, revision, slots } = useStore(store);
  return <><FrameView frame={frame} revision={revision} />{slots.map(entry => <SlotView key={entry.key} slot={entry} />)}</>;
}
function BodySlot({ body }: { body: ChatBody }) {
  const host = useRef<HTMLDivElement>(null);
  const scrollbar = useStore(conversationAppearance, state => state.scrollbar);
  useLayoutEffect(() => { const slot = host.current; slot?.prepend(body); return () => body.remove(); }, [body]);
  return <div ref={host} className="agbodyw">{scrollbar && <ConversationRail body={body}/>}</div>;
}

/// The conversation's scrollbar: where the view is, and a tick at each message
/// the user sent. A tick jumps to its message; the thumb drags.
interface RailView { thumb: { top: number; height: number } | null; ticks: { key: string; top: number; prompt: string }[] }
const NO_RAIL: RailView = { thumb: null, ticks: [] };
/// The thumb's least height, in pixels, so a long log keeps one to grab.
const THUMB_MIN = 24;

function ConversationRail({ body }: { body: ChatBody }) {
  const rail = useRef<HTMLDivElement>(null);
  const promptMarksEnabled = useStore(conversationAppearance, state => state.promptMarks);
  const [view, setView] = useState(NO_RAIL);
  const [peek, setPeek] = useState<string | null>(null);

  useEffect(() => {
    let frame = 0;
    const read = () => {
      frame = 0;
      const box = rail.current, total = body.scrollHeight, shown = body.clientHeight;
      if (!box || total <= shown + 1) return setView(NO_RAIL);
      const height = box.clientHeight, scale = height / total;
      const thumbHeight = Math.max(THUMB_MIN, shown * scale);
      const thumbTop = (body.scrollTop / (total - shown)) * (height - thumbHeight);
      const ticks = (promptMarksEnabled ? promptMarks(body) : []).map(mark => ({ key: mark.key, prompt: mark.prompt, top: Math.min(height - 2, mark.top * scale) }));
      setView({ thumb: { top: thumbTop, height: thumbHeight }, ticks });
    };
    const later = () => { frame ||= requestAnimationFrame(read); };
    const sized = new ResizeObserver(later);
    sized.observe(body);
    body.addEventListener("scroll", later, { passive: true });
    const drawn = onLogDrawn(body, later);
    later();
    return () => { cancelAnimationFrame(frame); sized.disconnect(); body.removeEventListener("scroll", later); drawn(); };
  }, [body, promptMarksEnabled]);

  const jump = (key: string) => {
    revealLogUnit(body, key);
    syncJump(body);
  };

  // A press on the track brings that part of the log into view; one on the thumb drags it.
  const press = (event: ReactPointerEvent<HTMLDivElement>) => {
    const box = rail.current, thumb = view.thumb;
    if (!box || !thumb || event.button !== 0 || (event.target as HTMLElement).closest(".tk")) return;
    event.preventDefault();
    const top = box.getBoundingClientRect().top, travel = box.clientHeight - thumb.height;
    const room = body.scrollHeight - body.clientHeight;
    const onThumb = event.clientY - top >= thumb.top && event.clientY - top <= thumb.top + thumb.height;
    const grab = onThumb ? event.clientY - top - thumb.top : thumb.height / 2;
    const follow = (y: number) => {
      body.stuck = false;
      body.scrollTop = travel > 0 ? ((y - top - grab) / travel) * room : 0;
    };
    if (!onThumb) follow(event.clientY);
    const target = event.currentTarget;
    target.setPointerCapture(event.pointerId);
    const move = (moved: PointerEvent) => follow(moved.clientY);
    const up = () => { target.removeEventListener("pointermove", move); target.removeEventListener("pointerup", up); target.removeEventListener("pointercancel", up); };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
    target.addEventListener("pointercancel", up);
  };

  // The message under the pointer, beside its tick.
  const ticks = promptMarksEnabled ? view.ticks : [];
  const shown = ticks.find(tick => tick.key === peek) ?? null;

  return <div className={view.thumb ? "agrail" : "agrail off"} ref={rail} onPointerDown={press}>
    {view.thumb && <div className="th" style={{ top: view.thumb.top, height: view.thumb.height }}/>}
    {ticks.map(tick => <button type="button" className="tk" key={tick.key} style={{ top: tick.top }} aria-label={`Jump to your message: ${tick.prompt.slice(0, 80)}`}
      onClick={() => jump(tick.key)} onPointerEnter={() => setPeek(tick.key)} onPointerLeave={() => setPeek(null)} onFocus={() => setPeek(tick.key)} onBlur={() => setPeek(null)}/>)}
    {shown && <div className="agpeek" style={{ top: shown.top }} aria-hidden="true">{shown.prompt}</div>}
  </div>;
}

const AgentHints = memo(function AgentHints({ value }: { value: AgentHintsModel }) {
  const sections = SHORTCUTS.filter(section => ["Agent session", "Side pane", "Terminals"].includes(section.title));
  return <div className="agkeys"><HintView value={value.value} slim /><StashIndicator project={value.project} /><details id="agshortcuts"><summary>Show more</summary><div className="agkeys-list">{sections.map(section => <section key={section.title}><b>{section.title}</b>{section.rows.map(row => <span key={`${row.keys}:${row.does}`}><kbd>{row.keys}</kbd>{row.does}</span>)}</section>)}<StashActions /></div></details></div>;
});

const FrameView = memo(function FrameView({ frame, revision }: { frame: Frame; revision: number }) {
  return <><div className="dhead ag-head" data-tauri-drag-region="" key={`head:${revision}`}>{frame.head}</div><PaneShell><PaneMain><div className="agconv" id="agconv" key={`conversation:${revision}`}><BodySlot body={frame.body} />{frame.field}<AgentHints value={frame.hints} /></div><PaneDock dock="bottom" /></PaneMain><PaneDock dock="right" /><PaneDropZone /></PaneShell></>;
});

