import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { placeMenuAt } from "../../stores/agents/agentpick.ts";

export type ChoiceAction = "agm" | "agf" | "agk" | "agw" | "agtype" | "agb" | "agbtype" | "aga";
export type ChoiceRow = { kind: "rule" } | { kind: "button"; action: ChoiceAction | null; value: string; label: ReactNode; detail: string; selected?: boolean; disabled?: boolean; dot?: "on" | "off"; mono?: boolean };
export interface ChoiceMenu { style: "" | "cols" | "folder" | "base"; rows: ChoiceRow[] }
interface Place { top: number; left: number; maxHeight: number; up: boolean }

/// The open menu of the new-agent form, floating over the page by its pill so
/// nothing clips it: below when it fits in the window, else above, scrolling
/// inside.
export function ChoiceMenuView({ menu, anchor, pick }: { menu: ChoiceMenu | null; anchor(): HTMLElement | null; pick(action: ChoiceAction, value: string): void }) {
  const node = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<Place | null>(null);

  useLayoutEffect(() => {
    if (!menu) return;
    const measure = () => {
      const element = node.current, pill = anchor();
      if (!element || !pill?.isConnected) return;
      const next = placeMenuAt(pill.getBoundingClientRect(), { width: element.offsetWidth, height: element.scrollHeight }, { top: 0, bottom: window.innerHeight, width: window.innerWidth });
      setPlace(prior => prior && prior.top === next.top && prior.left === next.left && prior.maxHeight === next.maxHeight && prior.up === next.up ? prior : next);
    };
    measure();
    // It follows its pill when the form scrolls or the window resizes.
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, { capture: true, passive: true });
    return () => {
      window.removeEventListener("resize", measure);
      window.removeEventListener("scroll", measure, { capture: true });
    };
  }, [menu, anchor]);

  const style = place ? { top: `${place.top}px`, left: `${place.left}px`, maxHeight: `${place.maxHeight}px` } : undefined;
  return <div id="agunder">{menu && <div ref={node} className={`agc-menu${menu.style ? ` ${menu.style}` : ""} float${place?.up ? " up" : ""}`} role="menu" style={style}>{menu.rows.map((row, index) => row.kind === "rule" ? <hr key={`rule:${index}`} /> : <button type="button" key={`${row.action}:${row.value}`} {...(row.action ? { [`data-${row.action}`]: row.value } : {})} className={row.selected ? "on" : ""} disabled={row.disabled} onClick={() => { if (row.action) pick(row.action, row.value); }}><span className={row.dot ? `dot${row.dot === "off" ? " off" : ""}` : undefined} />{row.label}<small className={row.mono ? "mono" : undefined}>{row.detail}</small></button>)}</div>}</div>;
}
