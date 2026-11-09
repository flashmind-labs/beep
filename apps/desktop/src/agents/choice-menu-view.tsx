import type { ReactNode } from "react";

type Action = "agp" | "agm" | "agf" | "agk" | "agw" | "agtype" | "agb" | "agbtype";
export type ChoiceRow = { kind: "rule" } | { kind: "button"; action: Action | null; value: string; label: ReactNode; detail: string; selected?: boolean; disabled?: boolean; dot?: "on" | "off"; mono?: boolean };
export interface ChoiceMenu { style: "" | "cols" | "folder" | "base"; rows: ChoiceRow[] }

export function ChoiceMenuView({ menu }: { menu: ChoiceMenu | null }) {
  return <div id="agunder">{menu && <div className={`agc-menu${menu.style ? ` ${menu.style}` : ""}`} role="menu">{menu.rows.map((row, index) => row.kind === "rule" ? <hr key={`rule:${index}`} /> : <button type="button" key={`${row.action}:${row.value}`} {...(row.action ? { [`data-${row.action}`]: row.value } : {})} className={row.selected ? "on" : ""} disabled={row.disabled}><span className={row.dot ? `dot${row.dot === "off" ? " off" : ""}` : undefined} />{row.label}<small className={row.mono ? "mono" : undefined}>{row.detail}</small></button>)}</div>}</div>;
}
