import type { Hint } from "../../stores/bar/hints.ts";

export interface HintViewModel {
  items: readonly Hint[];
  window: boolean;
}

export function HintView({ value, slim = false }: { value: HintViewModel; slim?: boolean }) {
  return <div className={`dhints${slim ? " slim" : ""}`}>{value.items.map(([key, text], index) => <span className={index >= 2 ? "lo" : undefined} key={`${key}:${index}`}><kbd dangerouslySetInnerHTML={{ __html: key }} />{text}</span>)}{value.window && <button className="win" onClick={openInWindow}><kbd>⌘O</kbd>open in window</button>}</div>;
}

// The hints module loads with the first click, so a view drawn alone keeps a small graph.
const openInWindow = () => void import("../../stores/bar/hints.ts").then(hints => hints.openInWindow());
