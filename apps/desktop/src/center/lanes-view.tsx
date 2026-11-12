// The board: Needs you, Working, Ready to review and Done today. Home shows
// every project's cards and a project page its own.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { LineRow, onRowKey } from "../ui/kit.tsx";
import { heroCard, motionAllowed, movePlaying, setHeroCard, withMove } from "../ui/motion.ts";
import { LANES, byLane, travelled, type Lane } from "./lanes-state.ts";

export interface BoardCard {
  key: string;
  lane: Lane;
  lead: ReactNode;
  title: string;
  sub: ReactNode;
  foot: ReactNode;
  /// The one button of a card that waits on the user.
  action: { label: string; primary: boolean; run(): void } | null;
  /// True for a session, whose card grows into the session's head.
  session: boolean;
  dim: boolean;
  hint: string | null;
  open(): void;
  menu: ((event: MouseEvent) => void) | null;
}

/// How Home and a project's front page lay out, each on its own: the board, or the
/// sections and rows before it. Picked in Settings, under Appearance.
export type FrontLayout = "board" | "list";
export type FrontPage = "home" | "project";
const layoutKey = (page: FrontPage) => `beep.layout.${page}`;
const kept = (page: FrontPage): FrontLayout => { try { return localStorage.getItem(layoutKey(page)) === "list" ? "list" : "board"; } catch { return "board"; } };
const layouts: Record<FrontPage, FrontLayout> = { home: kept("home"), project: kept("project") };
export const frontLayout = (page: FrontPage) => layouts[page];
/// Called when the user picks a layout: every page that shows it draws again.
export const layoutChanged = new Set<() => void>();

export function setFrontLayout(page: FrontPage, next: FrontLayout) {
  if (next === layouts[page]) return;
  layouts[page] = next;
  try { localStorage.setItem(layoutKey(page), next); } catch { /* kept for this launch only */ }
  for (const draw of layoutChanged) draw();
}

/// A card as a row of a list section. A session's row opens with the page move.
export function CardRow({ card }: { card: BoardCard }) {
  const action = card.action;
  const open = card.session ? () => { setHeroCard(null); withMove("open", card.open); } : card.open;
  return <LineRow lead={card.lead} title={card.title} sub={card.sub} dim={card.dim} rowTitle={card.hint ?? undefined} onOpen={open} onContext={card.menu ?? undefined}
    right={action ? <button type="button" className={`dbtn ${action.primary ? "pri " : ""}small`} onClick={event => { event.stopPropagation(); action.run(); }}>{action.label}</button> : card.foot}/>;
}

/// `more` makes a lane's title open the full list behind it.
export function Board({ cards, more }: { cards: readonly BoardCard[]; more: Partial<Record<Lane, () => void>> }) {
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

  return <div className="bd-lanes" ref={box}>{LANES.map(({ lane, title }) => <div className="bd-lane" key={lane}>
    <h4>{more[lane] ? <button type="button" onClick={more[lane]}>{title}</button> : title}<span>{lanes[lane].length || ""}</span></h4>
    {lanes[lane].length ? lanes[lane].map(card => <Card key={card.key} card={card} open={() => open(card)}/>) : <div className="bd-empty">Nothing here</div>}
  </div>)}</div>;
}

function Card({ card, open }: { card: BoardCard; open(): void }) {
  const style = { opacity: card.dim ? .55 : undefined, viewTransitionName: heroCard() === card.key ? "board-hero" : undefined };
  const action = card.action;

  return <div className="bd-card" data-card={card.key} role="button" tabIndex={0} title={card.hint ?? undefined} style={style}
    onClick={open} onKeyDown={onRowKey(open)} onContextMenu={card.menu ? event => card.menu?.(event.nativeEvent) : undefined}>
    <b>{card.title}</b>
    <span className="sub">{card.lead}<span>{card.sub}</span></span>
    {(card.foot || action) && <span className="ft">{card.foot}<span className="sp"/>
      {action && <button type="button" className={`dbtn small${action.primary ? " pri" : ""}`} onClick={event => { event.stopPropagation(); action.run(); }}>{action.label}</button>}
    </span>}
  </div>;
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
