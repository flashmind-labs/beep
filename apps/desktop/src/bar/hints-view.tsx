import type { Hint, HintAction } from "./hints.ts";

export interface HintViewModel {
  items: readonly Hint[];
  window: boolean;
  model: string | null;
  link: readonly ["off" | "bad", string] | null;
}

export function HintView({ value, slim = false }: { value: HintViewModel; slim?: boolean }) {
  return <div className={`dhints${slim ? " slim" : ""}`}>{value.items.map(([key, text], index) => <span className={index >= 2 ? "lo" : undefined} key={`${key}:${index}`}><kbd dangerouslySetInnerHTML={{ __html: key }} />{text}</span>)}{value.window && <button className="win" onClick={() => act("window")}><kbd>⌘O</kbd>open in window</button>}{value.model && <button className="dmodel" onClick={() => act("model")}>{value.model}<kbd>⌘E</kbd></button>}{value.link && <button className="st" onClick={() => act("link")}><span className={`ddot ${value.link[0]}`} />{value.link[1]}</button>}</div>;
}

// The bar's chat modules load with the first click, so a view drawn alone keeps a small graph.
const act = (action: HintAction) => void import("./hints.ts").then(hints => hints.runHint(action));
