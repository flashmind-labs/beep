import { useLayoutEffect, useState } from "react";
import { turnElement, turnsOf, viewOf, type Turn } from "./transcript.ts";
import { askText } from "./turns.ts";
import { focusAsk, scrollDown } from "./chat.ts";
import { drawThrough, earlierPrompts } from "./history.ts";
import { ICON } from "../ui/icons.tsx";
import type { ChatBody } from "../ui/turn.ts";

const NAVIGATION_CHANGED = "chat-navigation-changed";
export function updateChatNavigation(body: ChatBody) { body.dispatchEvent(new window.Event(NAVIGATION_CHANGED)); }
interface Tick { key: string; title: string; top: number; active: boolean; turn: HTMLElement | null; earlier: number | null }
interface Navigation { latest: boolean; top: number; height: number; many: boolean; ticks: Tick[] }

/// The turns of `body` with a prompt.
const prompted = (body: ChatBody): Turn[] => turnsOf(body).filter(turn => viewOf(turn).prompt !== null);

function readNavigation(body: ChatBody): Navigation {
  const turns = prompted(body).flatMap(turn => { const element = turnElement(turn); return element ? [{ turn, element }] : []; });
  const earlier = earlierPrompts(body), total = earlier.length + turns.length;
  let current = earlier.length;
  turns.forEach(({ element }, index) => { if (element.offsetTop - body.offsetTop <= body.scrollTop + body.clientHeight / 3) current = earlier.length + index; });
  const place = (position: number) => Math.min(97, Math.max(3, position / total * 100));
  const visible = turns.length >= 2 && body.scrollHeight > body.clientHeight + 4;
  const ticks: Tick[] = visible ? [
    ...earlier.map((title, index) => ({ key: `earlier:${index}`, title, top: place(index), active: index === current, turn: null, earlier: index })),
    ...turns.map(({ turn, element }, index) => ({ key: `turn:${turn.key}`, title: askText(turn), top: place(earlier.length + (element.offsetTop - body.offsetTop) / body.scrollHeight * turns.length), active: earlier.length + index === current, turn: element, earlier: null })),
  ] : [];
  const latest = body.stuck === false, top = body.offsetTop, height = body.clientHeight, many = total > 12;
  return { latest, top, height, many, ticks };
}

function sameNavigation(a: Navigation | null, b: Navigation) {
  return !!a && a.latest === b.latest && a.top === b.top && a.height === b.height && a.many === b.many && a.ticks.length === b.ticks.length && a.ticks.every((tick, index) => {
    const next = b.ticks[index];
    return tick.key === next.key && tick.title === next.title && tick.top === next.top && tick.active === next.active;
  });
}

export function ChatNavigation({ body }: { body: ChatBody }) {
  const [navigation, setNavigation] = useState<Navigation | null>(null);
  useLayoutEffect(() => {
    let frame = 0;
    const schedule = () => { if (!frame) frame = requestAnimationFrame(() => { frame = 0; const next = readNavigation(body); setNavigation(previous => sameNavigation(previous, next) ? previous : next); }); };
    body.addEventListener(NAVIGATION_CHANGED, schedule);
    schedule();
    return () => { body.removeEventListener(NAVIGATION_CHANGED, schedule); if (frame) cancelAnimationFrame(frame); };
  }, [body]);
  if (!navigation) return null;
  const jump = (tick: Tick) => {
    if (tick.earlier !== null) {
      drawThrough(body, tick.earlier);
      const turn = prompted(body)[tick.earlier], element = turn ? turnElement(turn) : null;
      if (element) body.scrollTop = element.offsetTop - body.offsetTop - 6;
    } else if (tick.turn) body.scrollTo({ top: tick.turn.offsetTop - body.offsetTop - 6, behavior: "smooth" });
    focusAsk();
  };
  return <>{navigation.latest && <button className="dbtn dlatest" id="latest" onClick={() => { scrollDown(); updateChatNavigation(body); focusAsk(); }}><span className="down" dangerouslySetInnerHTML={{ __html: ICON.chev }} />Latest</button>}{!!navigation.ticks.length && <div className={`dmap${navigation.many ? " many" : ""}`} id="map" aria-label="Prompts in this chat" style={{ top: navigation.top, height: navigation.height }}>{navigation.ticks.map(tick => <button className={`tk${tick.active ? " on" : ""}`} key={tick.key} style={{ top: `${tick.top}%` }} aria-label={tick.title} onClick={() => jump(tick)}><i /><span className="tip">{tick.title}</span></button>)}</div>}</>;
}
