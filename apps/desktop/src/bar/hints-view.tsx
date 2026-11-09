import type { Hint } from "./hints.ts";

export interface HintViewModel {
  items: readonly Hint[];
  window: boolean;
  model: string | null;
  link: readonly ["off" | "bad", string] | null;
}

export function HintView({ value }: { value: HintViewModel }) {
  return <div className="dhints">{value.items.map(([key, text], index) => <span className={index >= 2 ? "lo" : undefined} key={`${key}:${index}`}><kbd dangerouslySetInnerHTML={{ __html: key }} />{text}</span>)}{value.window && <button className="win" data-act="openWindow"><kbd>⌘O</kbd>open in window</button>}{value.model && <button className="dmodel" data-act="model">{value.model}<kbd>⌘E</kbd></button>}{value.link && <button className="st"><span className={`ddot ${value.link[0]}`} />{value.link[1]}</button>}</div>;
}
