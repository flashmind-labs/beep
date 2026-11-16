// The board: Needs you, Working, Ready to review and Done today. Home shows
// every project's cards, a row for each project, and a project page its own,
// with the sessions that ended as a list.
import { Fragment, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { Icon } from "../ui/icons.tsx";
import { Button, LineRow, onRowKey } from "../ui/kit.tsx";
import { openMenu, type MenuAction } from "../ui/menu.tsx";
import { heroCard, motionAllowed, movePlaying, setHeroCard, withMove } from "../ui/motion.ts";
import { isMobile } from "../ui/platform.ts";
import { LANES, byLane, byProject, travelled, withoutProject, type Lane } from "./lanes-state.ts";

export interface BoardCard {
  key: string;
  lane: Lane;
  /// The project whose row holds it on Home; null for a card of no project.
  project: string | null;
  lead: ReactNode;
  title: string;
  sub: ReactNode;
  foot: ReactNode;
  /// The one button of a card that waits on the user.
  action: CardAction | null;
  /// True for a session, whose card grows into the session's head.
  session: boolean;
  dim: boolean;
  hint: string | null;
  open(): void;
  menu: ((event: MouseEvent) => void) | null;
}

/// What a card's button does: one thing, or a menu of them.
type CardAction =
  | { kind: "run"; label: string; primary: boolean; run(): void }
  | { kind: "menu"; label: string; items: readonly MenuAction[] };

function ActionButton({ action, title }: { action: CardAction; title: string }) {
  if (action.kind === "menu") {
    return <Button small className="bd-pick" aria-haspopup="menu" onClick={event => { event.stopPropagation(); openMenu(event.nativeEvent, title, action.items); }}>{action.label}<Icon name="caretDown"/></Button>;
  }

  return <Button look={action.primary ? "primary" : "secondary"} small onClick={event => { event.stopPropagation(); action.run(); }}>{action.label}</Button>;
}

/// Home's rows, a project each: its dot's color, and what its name opens; null when it has neither.
export type ProjectRows = (project: string) => { color: string | null; open: (() => void) | null };

/// A card as a row of a list section. A session's row opens with the page move.
export function CardRow({ card }: { card: BoardCard }) {
  const action = card.action;
  const open = card.session ? () => { setHeroCard(null); withMove("open", card.open); } : card.open;
  return <LineRow lead={card.lead} title={card.title} sub={card.sub} dim={card.dim} rowTitle={card.hint ?? undefined} onOpen={open} onContext={card.menu ?? undefined}
    right={action ? <ActionButton action={action} title={card.title}/> : card.foot}/>;
}

/// `more` makes a lane's title open the full list behind it. `rows` gives
/// each project a row of its own under the lanes' titles. Without rows, ended
/// sessions are a short list, and `beside` fills the room beside the lanes in
/// a wide window. Lanes stay put; only the cards move.
export function Board({ cards, more, rows, recency = null, beside = null }: { recency?: ReadonlyMap<string, number> | null; cards: readonly BoardCard[]; more: Partial<Record<Lane, () => void>>; rows: ProjectRows | null; beside?: ReactNode }) {
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

  if (rows) {
    return <div className="bd-rows" ref={box}>
      {LANES.map(({ lane, title }) => <div className="bd-lane hd" key={lane}>{heading(lane, title)}</div>)}
      {byProject(cards, recency).map(row => { const own = byLane(row.cards), name = row.project ?? "Other", head = row.project === null ? { color: null, open: null } : rows(row.project); return <Fragment key={row.project ?? ""}>
        <h3 className="pj" title={name}>{head.color && <span className="cc-pdot" style={{ background: head.color }}/>}{head.open ? <button type="button" onClick={head.open}>{name}</button> : <span>{name}</span>}</h3>
        {LANES.map(({ lane }) => <Cell key={lane} shown="two">{own[lane].map(card => <Card key={card.key} card={typeof card.sub === "string" ? { ...card, sub: withoutProject(card.sub, row.project) } : card} open={() => open(card)}/>)}</Cell>)}
      </Fragment>; })}
    </div>;
  }

  return <div className="bd-lanes" ref={box}>{LANES.map(({ lane, title }) => lane === "done"
    ? <div className="bd-lane log" key={lane}>
      {heading(lane, title)}
      <div className="bd-log">{lanes.done.slice(0, DONE_SHOWN).map(card => <CardRow key={card.key} card={{ ...card, sub: "" }}/>)}</div>
      {lanes.done.length > DONE_SHOWN && more.done && <Button look="quiet" small onClick={more.done}>All {lanes.done.length}</Button>}
    </div>
    : <div className="bd-lane" key={lane}>
      {heading(lane, title)}
      <Cell shown="page">{lanes[lane].map(card => <Card key={card.key} card={card} open={() => open(card)}/>)}</Cell>
    </div>)}{beside !== null && <div className="bd-art">{beside}</div>}</div>;
}

/// How many ended sessions the done log lists before its "All" button.
const DONE_SHOWN = 10;

function Card({ card, open }: { card: BoardCard; open(): void }) {
  const style = { opacity: card.dim ? .55 : undefined, viewTransitionName: heroCard() === card.key ? "board-hero" : undefined };
  const action = card.action;

  return <div className="bd-card" data-card={card.key} role="button" tabIndex={0} title={card.hint ?? undefined} style={style}
    onClick={open} onKeyDown={onRowKey(open)} onContextMenu={card.menu ? event => card.menu?.(event.nativeEvent) : undefined}>
    <b>{card.title}</b>
    <span className="sub">{card.lead}<span>{card.sub}</span></span>
    {(card.foot || action) && <span className="ft">{card.foot}<span className="sp"/>
      {action && <ActionButton action={action} title={card.title}/>}
    </span>}
  </div>;
}

/// How many whole cards a lane's cell shows: two in a project's row on Home,
/// or as many as fit above the bottom of a project's page.
type Shown = "two" | "page";

/// A lane's cards, which scroll a card at a time, as every list of cards does
/// (`itemScroll`).
function Cell({ shown, children }: { shown: Shown; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [fit, setFit] = useState<number | null>(null);

  // The cell is as tall as its shown cards, so none shows cut at the bottom.
  useLayoutEffect(() => { if (ref.current) setFit(shownHeight(ref.current, shown)); });

  // A cell that fills the page fits again when the page changes size.
  useEffect(() => {
    const cell = ref.current, page = cell && shown === "page" ? pageOf(cell) : null;
    if (!cell || !page) return;
    const observer = new ResizeObserver(() => setFit(shownHeight(cell, shown)));

    observer.observe(page);
    return () => observer.disconnect();
  }, [shown]);

  return <div className={fit === null ? "bd-lane cell" : "bd-lane cell full"} ref={ref} style={fit === null ? undefined : { maxHeight: fit }}>{children}</div>;
}

/// The fewest cards a cell shows at once.
const SHOWN = 2;

/// The height of the shown cards and the gaps between them, or null when the cell holds no more.
function shownHeight(cell: HTMLElement, shown: Shown) {
  const cards = [...cell.children].flatMap(card => card instanceof HTMLElement ? [card.offsetHeight] : []);
  const gap = Number.parseFloat(getComputedStyle(cell).rowGap) || 0, tallest = Math.max(...cards);
  const count = shown === "page" ? pageFits(cell, tallest + gap) : SHOWN;
  if (count === null || cards.length <= count) return null;

  return count * tallest + (count - 1) * gap;
}

/// How many cards of `step` pixels fit between the cell's top and the bottom
/// of its page, at least `SHOWN`; null on a phone, whose lanes show every card.
function pageFits(cell: HTMLElement, step: number) {
  const page = pageOf(cell);
  if (!page || isMobile()) return null;
  const room = page.clientHeight - (cell.getBoundingClientRect().top - page.getBoundingClientRect().top + page.scrollTop);

  return Math.max(SHOWN, Math.floor(room / step));
}

/// The project page that scrolls a board's lanes.
function pageOf(cell: HTMLElement) {
  return cell.closest<HTMLElement>(".pj-board");
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
