// The board: Needs you, Working, Ready to review and Done today, with the
// sessions that ended as a list. Home leaves Needs you to its queue.
import type { ProjectWindowProps } from "../../stores/project/projectwindow.ts";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { Icon } from "../../ui/icons.tsx";
import { AgentButton, type AgentButtonItem } from "../../parts/review/feedback-view.tsx";
import { Button, LineRow, onRowKey } from "../../ui/kit.tsx";
import { openMenu, type MenuAction } from "../../ui/menu.tsx";
import { heroCard, motionAllowed, movePlaying, setHeroCard, withMove } from "../../ui/motion.ts";
import { isMobile, onPhone } from "../../ui/platform.ts";
import { invoke } from "../../ui/tauri.ts";
import type { AgentQuestion } from "../../stores/agents/running-state.ts";
import { LANES, byLane, travelled, type Lane } from "../../stores/center/lanes-state.ts";

export interface BoardCard {
  windowProps?: ProjectWindowProps;
  key: string;
  lane: Lane;
  /// The project it belongs to; null for a card of no project.
  project: string | null;
  /// Its project's colour, shown as a dot before the muted line; null shows none.
  color: string | null;
  lead: ReactNode;
  title: string;
  sub: ReactNode;
  foot: ReactNode;
  /// The one button of a card that waits on the user.
  action: CardAction | null;
  /// True for a session, whose card grows into the session's head.
  session: boolean;
  dim: boolean;
  /// A teammate's card, a shade dimmer than the user's own.
  theirs: boolean;
  hint: string | null;
  open(): void;
  menu: ((event: MouseEvent) => void) | null;
}

/// What a card's button does: one thing, a menu of them, an agent button
/// whose model pick starts a session, the answers to an agent's question,
/// closing the session, or several of these side by side.
export type CardAction =
  | { kind: "run"; label: string; primary: boolean; run(): void; windowProps?: ProjectWindowProps }
  | { kind: "several"; actions: readonly CardAction[] }
  | { kind: "menu"; label: string; items: readonly MenuAction[] }
  | { kind: "agent"; item: AgentButtonItem }
  | { kind: "answer"; question: AgentQuestion; answer(reply: string): Promise<void> }
  | { kind: "close"; run(): void };

function ActionButton({ action, title }: { action: CardAction; title: string }) {
  switch (action.kind) {
    case "menu": return <Button small className="bd-pick" aria-haspopup="menu" onClick={event => { event.stopPropagation(); openMenu(event.nativeEvent, title, action.items); }}>{action.label}<Icon name="caretDown"/></Button>;
    // Clicks in its picker stay off the card.
    case "agent": return <span style={{ display: "contents" }} onClick={event => event.stopPropagation()}><AgentButton item={action.item} /></span>;
    case "run": return <Button {...action.windowProps?.clicks} {...action.windowProps?.menu} look={action.primary ? "primary" : "secondary"} small onClick={event => { event.stopPropagation(); action.run(); }}>{action.label}</Button>;
    case "answer": return <Answers key={action.question.id} question={action.question} answer={action.answer}/>;
    case "close": return <Button look="quiet" small className="bd-close" onClick={event => { event.stopPropagation(); action.run(); }}>Close</Button>;
    case "several": return <span className="bd-acts">{action.actions.map((each, index) => <ActionButton key={index} action={each} title={title}/>)}</span>;
  }
}

/// A question's options as buttons: the first that allows is the primary one,
/// and those that deny are marked. They stay disabled while the answer is on
/// its way, and come back if it fails. A phone leaves them out: its row opens the chat.
function Answers({ question, answer }: { question: AgentQuestion; answer(reply: string): Promise<void> }) {
  const [sending, setSending] = useState(false);
  const allow = question.options.findIndex(option => option.allow);

  const pick = (reply: string) => {
    setSending(true);
    answer(reply).catch(error => {
      console.warn("answer:", error);
      setSending(false);
    });
  };

  return <span className="bd-answers">{question.options.map((option, index) => <Button key={option.id} small title={option.name} disabled={sending} look={index === allow ? "primary" : "secondary"} danger={!option.allow}
    onClick={event => { event.stopPropagation(); pick(option.id); }}>{option.name}</Button>)}</span>;
}

/// The dot of the card's project, before its muted line; null without a colour.
function dot(card: BoardCard) {
  return card.color === null ? null : <span className="cc-pdot" style={{ background: card.color }}/>;
}

/// A card as a row of a list section. A session's row opens with the page move.
export function CardRow({ card }: { card: BoardCard }) {
  const action = card.action;
  const open = card.session ? () => { setHeroCard(null); withMove("open", card.open); } : card.open;
  return <LineRow events={{ ...card.windowProps?.clicks, ...card.windowProps?.menu }} lead={card.lead} title={card.title} sub={<>{dot(card)}{card.sub}</>} dim={card.dim} rowTitle={card.hint ?? undefined} onOpen={open} onContext={card.menu ?? undefined}
    right={action ? <ActionButton action={action} title={card.title}/> : card.foot}/>;
}

/// `more` makes a lane's title open the full list behind it, and `lanes`
/// picks the lanes drawn. Ended sessions are a short list, and `beside` fills
/// the room beside the lanes in a wide window. With `spread`, each lane lays its
/// cards out in its `COLUMNS` of one width, fewer when it has fewer cards, and an
/// empty lane keeps to its title. Lanes stay put; only the cards move.
export function Board({ cards, more, lanes: drawn = LANES, beside = null, spread = false }: { cards: readonly BoardCard[]; more: Partial<Record<Lane, () => void>>; lanes?: typeof LANES; beside?: ReactNode; spread?: boolean }) {
  const lanes = byLane(cards);
  const box = useTravel(cards);
  const [, redraw] = useState(0);

  const open = (card: BoardCard) => {
    if (!card.session) return card.open();
    // The card takes the shared name before the page is captured.
    setHeroCard(card.key);
    flushSync(() => redraw(n => n + 1));
    withMove("open", card.open);
  };

  const heading = (lane: Lane, title: string) => <h4>{more[lane] ? <button type="button" onClick={more[lane]}>{title}</button> : title}<span>{lanes[lane].length || ""}</span></h4>;

  // A spread lane's columns, which are also its share of the width.
  const columns = (lane: Lane) => spread ? Math.min(COLUMNS[lane], lanes[lane].length) : null;
  const share = (lane: Lane) => spread ? { flexGrow: columns(lane) ?? 0 } : undefined;
  const named = (name: string, lane: Lane) => spread && !lanes[lane].length ? `${name} empty` : name;

  return <div className={spread ? "bd-lanes spread" : "bd-lanes"} ref={box}>{drawn.map(({ lane, title }) => lane === "done"
    ? <div className={named("bd-lane log", lane)} key={lane} style={share(lane)}>
      {heading(lane, title)}
      <div className="bd-log">{lanes.done.slice(0, DONE_SHOWN).map(card => <CardRow key={card.key} card={{ ...card, sub: "", color: null }}/>)}</div>
      {lanes.done.length > DONE_SHOWN && more.done && <Button look="quiet" small onClick={more.done}>All {lanes.done.length}</Button>}
    </div>
    : <div className={named(lanes[lane].length > CROWDED_FROM ? "bd-lane many" : "bd-lane", lane)} key={lane} style={share(lane)}>
      {heading(lane, title)}
      <Cell columns={columns(lane)}>{lanes[lane].map(card => <Card key={card.key} card={card} open={() => open(card)}/>)}</Cell>
    </div>)}{beside !== null && <div className="bd-art">{beside}</div>}</div>;
}

/// How many ended sessions the done log lists before its "All" button.
const DONE_SHOWN = 10;
/// A lane with more cards than this takes the painting's room for wider cards. A count, not
/// the cell's fit: the fit depends on the lane's width, which the painting changes.
const CROWDED_FROM = 4;
/// The columns of a spread lane: Working one wide column, Ready to review two, and
/// Done today its rows.
const COLUMNS: Record<Lane, number> = { needs: 1, working: 1, review: 2, done: 1 };

export function Card({ card, open }: { card: BoardCard; open(): void }) {
  const style = { opacity: card.dim ? .55 : undefined, viewTransitionName: heroCard() === card.key ? "board-hero" : undefined };
  const action = card.action;

  return <div {...card.windowProps?.clicks} {...card.windowProps?.menu} className={card.theirs ? "bd-card theirs" : "bd-card"} data-card={card.key} role="button" tabIndex={0} title={card.hint ?? undefined} style={style}
    onClick={open} onKeyDown={onRowKey(open)} onContextMenu={card.menu ? event => card.menu?.(event.nativeEvent) : card.windowProps?.menu.onContextMenu}>
    <b>{card.title}</b>
    <span className="sub">{card.lead}{dot(card)}<span>{card.sub}</span></span>
    {(card.foot || action) && <span className="ft">{card.foot}<span className="sp"/>
      {action && <ActionButton action={action} title={card.title}/>}
    </span>}
  </div>;
}

/// A lane's cards, which scroll a card at a time, as every list of cards does
/// (`itemScroll`).
/// `columns` lays the cards out in that many columns of one width; null leaves it to the style.
function Cell({ columns, children }: { columns: number | null; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState<number | null>(null);

  // The cell is as tall as its shown cards, so none shows cut at the bottom. It measures
  // when its cards change, not on its own update: its height feeds back into the
  // measure, and measuring after every render could loop until React gives up.
  useLayoutEffect(() => { if (ref.current) setFit(shownHeight(ref.current)); }, [children]);

  // A cell that fills the page fits again when the page changes size.
  useEffect(() => {
    const cell = ref.current, page = cell ? pageOf(cell) : null;
    if (!cell || !page) return;
    const observer = new ResizeObserver(() => setFit(shownHeight(cell)));

    observer.observe(page);
    return () => observer.disconnect();
  }, []);

  // On a phone the cell snaps a card at a time, and ticks when it rests on another card.
  useEffect(() => {
    const cell = ref.current;
    if (!cell || !onPhone()) return;
    let rest = cell.scrollTop, timer: ReturnType<typeof setTimeout> | undefined;
    const settle = () => {
      if (Math.abs(cell.scrollTop - rest) < 1) return;

      rest = cell.scrollTop;
      // A phone without a haptic engine refuses, which changes nothing.
      invoke("plugin:haptics|selection_feedback").catch(() => undefined);
    };
    // Where the web view has no scrollend, a short pause in the scroll stands for it.
    const event = "onscrollend" in cell ? "scrollend" : "scroll";
    const listener = event === "scrollend" ? settle : () => { clearTimeout(timer); timer = setTimeout(settle, 100); };

    cell.addEventListener(event, listener, { passive: true });
    return () => { clearTimeout(timer); cell.removeEventListener(event, listener); };
  }, []);

  const style = { maxHeight: fit ?? undefined, gridTemplateColumns: columns === null ? undefined : `repeat(${columns}, minmax(0, 1fr))` };
  return <div className="bd-lane cell" ref={ref} style={style}>{children}</div>;
}

/// The fewest cards a cell shows at once.
const SHOWN = 2;

/// The height of the shown cards and the gaps between them, or null when the cell holds no more.
function shownHeight(cell: HTMLElement) {
  const cards = [...cell.children].flatMap(card => card instanceof HTMLElement ? [card.offsetHeight] : []);
  const gap = Number.parseFloat(getComputedStyle(cell).rowGap) || 0, tallest = Math.max(...cards);
  const count = pageFits(cell, tallest + gap);
  if (count === null || cards.length <= count) return null;

  return count * tallest + (count - 1) * gap;
}

/// How many cards of `step` pixels fit between the cell's top and the bottom
/// of its page, at least `SHOWN`; null on a phone, whose lanes show every card.
function pageFits(cell: HTMLElement, step: number) {
  const page = pageOf(cell);
  if (!page || isMobile()) return null;
  // Home's lanes stop above its view's bottom padding, so Home does not scroll; a project's board is left as it was.
  const padding = page.matches(".cc-view") ? Number.parseFloat(getComputedStyle(page).paddingBottom) || 0 : 0;
  const room = page.clientHeight - padding - (cell.getBoundingClientRect().top - page.getBoundingClientRect().top + page.scrollTop);

  return Math.max(SHOWN, Math.floor(room / step));
}

/// The page that scrolls a board's lanes: a project's board, or Home's view.
function pageOf(cell: HTMLElement) {
  return cell.closest<HTMLElement>(".pj-board, .cc-view");
}

type Places = Map<string, { x: number; y: number }>;

function places(box: HTMLElement) {
  const origin = box.getBoundingClientRect(), found: Places = new Map(), nodes = new Map<string, HTMLElement>();

  for (const node of box.querySelectorAll<HTMLElement>("[data-card]")) {
    const rect = node.getBoundingClientRect(), key = node.dataset.card ?? "";
    found.set(key, { x: rect.left - origin.left, y: rect.top - origin.top });
    nodes.set(key, node);
  }

  return { found, nodes };
}

/// A card that changes lane travels there. Layout is read only when a card
/// changed lane, and nothing plays under reduced motion or in a hidden window.
function useTravel(cards: readonly BoardCard[]) {
  const box = useRef<HTMLDivElement>(null);
  const last = useRef<{ lanes: string; before: Places | null; laneOf: Map<string, Lane> }>({ lanes: "", before: null, laneOf: new Map() });
  const lanes = cards.map(card => `${card.key}\n${card.lane}`).join("\n\n");

  if (lanes !== last.current.lanes) {
    const shown = box.current?.offsetParent ? box.current : null;
    last.current = { lanes, before: last.current.lanes && shown && motionAllowed() && !movePlaying() ? places(shown).found : null, laneOf: last.current.laneOf };
  }

  useLayoutEffect(() => {
    const before = last.current.before, target = box.current, was = last.current.laneOf;
    last.current.before = null;
    last.current.laneOf = new Map(cards.map(card => [card.key, card.lane]));
    if (!before || !target) return;
    const after = places(target);

    // Only a card that changed lane travels: one that a new card pushed down just moves.
    const moved = new Set(cards.filter(card => was.has(card.key) && was.get(card.key) !== card.lane).map(card => card.key));
    for (const [key, { dx, dy }] of travelled(before, after.found)) {
      if (!moved.has(key)) continue;
      after.nodes.get(key)?.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: 300, easing: "cubic-bezier(.2, 0, 0, 1)" });
    }
  });

  return box;
}
