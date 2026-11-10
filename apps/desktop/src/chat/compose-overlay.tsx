import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { Pill } from "./compose-text.ts";

export function ComposeOverlay({ value, pills, fence }: { value: string; pills: Pill[]; fence: { start: number; end: number } | null }) {
  const code = useRef<HTMLSpanElement>(null);
  const [outline, setOutline] = useState<{ top: number; height: number } | null>(null);
  useLayoutEffect(() => {
    const node = code.current, layer = node?.parentElement;
    const rects = node ? [...node.getClientRects()] : [];
    if (!layer || !rects.length) { setOutline(null); return; }
    const top = Math.min(...rects.map(rect => rect.top)), bottom = Math.max(...rects.map(rect => rect.bottom));
    const next = { top: top - layer.getBoundingClientRect().top + layer.scrollTop - 3, height: bottom - top + 6 };
    setOutline(previous => previous?.top === next.top && previous.height === next.height ? previous : next);
  });
  const run = (start: number, end: number) => {
    const result: ReactNode[] = [];
    let at = start;
    for (const pill of pills) {
      if (pill.start < start || pill.end > end) continue;
      result.push(value.slice(at, pill.start));
      result.push(<span key={`${pill.id}:${pill.start}`} className="cx-pill" data-id={pill.id}>{value.slice(pill.start, pill.end)}</span>);
      at = pill.end;
    }
    result.push(value.slice(at, end));
    return result;
  };
  return <>{fence ? <>{run(0, fence.start)}<span ref={code}>{run(fence.start, fence.end)}</span>{run(fence.end, value.length)}</> : run(0, value.length)}{" "}{outline && fence && <div className="cx-code" style={{ top: outline.top, height: outline.height }} />}</>;
}

export function ComposePopup({ text, lines, node, expand, remove }: { text: string; lines: number; node(element: HTMLDivElement | null): void; expand(): void; remove(): void }) {
  return <div className="cx-pop" ref={node}><div className="h"><b>Pasted text</b><span>{lines === 1 ? "1 line" : `${lines} lines`}</span><span className="sp" /><button type="button" className="dbtn quiet" onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); expand(); }}>Expand</button><button type="button" className="dbtn quiet" onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); remove(); }}>Remove</button></div><pre>{text}</pre></div>;
}
