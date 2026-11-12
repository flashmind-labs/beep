// The board: Needs you, Working, Ready to review and Done today. Home shows
// every project's cards and a project page its own.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { onRowKey } from "../ui/kit.tsx";
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
  const last = useRef<{ lanes: string; before: Places | null }>({ lanes: "", before: null });
  const lanes = cards.map(card => `${card.key}\n${card.lane}`).join("\n\n");

  if (lanes !== last.current.lanes) {
    const shown = box.current?.offsetParent ? box.current : null;
    last.current = { lanes, before: last.current.lanes && shown && motionAllowed() && !movePlaying() ? places(shown).found : null };
  }

  useLayoutEffect(() => {
    const before = last.current.before, target = box.current;
    last.current.before = null;
    if (!before || !target) return;
    const after = places(target);

    for (const [key, { dx, dy }] of travelled(before, after.found)) {
      after.nodes.get(key)?.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: 300, easing: "cubic-bezier(.2, 0, 0, 1)" });
    }
  });

  return box;
}
