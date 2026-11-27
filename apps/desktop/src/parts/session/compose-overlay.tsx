import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { Pill } from "../../stores/chat/compose-text.ts";
import { Button } from "../../ui/kit.tsx";
import { plural } from "../../ui/format.ts";

export function ComposeOverlay({ value, pills, fences, pillNode }: { value: string; pills: Pill[]; fences: { start: number; end: number }[]; pillNode(id: number, node: HTMLSpanElement | null): void }) {
  const code = useRef<(HTMLSpanElement | null)[]>([]);
  const [outlines, setOutlines] = useState<{ top: number; height: number }[]>([]);
  useLayoutEffect(() => {
    const next = fences.flatMap((_, i) => {
      const node = code.current[i], layer = node?.parentElement;
      const rects = node ? [...node.getClientRects()] : [];
      if (!layer || !rects.length) return [];
      const top = Math.min(...rects.map(rect => rect.top)), bottom = Math.max(...rects.map(rect => rect.bottom));
      return [{ top: top - layer.getBoundingClientRect().top + layer.scrollTop - 3, height: bottom - top + 6 }];
    });
    setOutlines(previous => previous.length === next.length && previous.every((o, i) => o.top === next[i].top && o.height === next[i].height) ? previous : next);
  });
  const run = (start: number, end: number) => {
    const result: ReactNode[] = [];
    let at = start;
    for (const pill of pills) {
      if (pill.start < start || pill.end > end) continue;
      result.push(value.slice(at, pill.start));
      result.push(<span key={`${pill.id}:${pill.start}`} className="cx-pill" ref={node => { pillNode(pill.id, node); }}>{value.slice(pill.start, pill.end)}</span>);
      at = pill.end;
    }
    result.push(value.slice(at, end));
    return result;
  };
  let at = 0;
  const parts = fences.flatMap((fence, i) => {
    const before = run(at, fence.start);
    at = fence.end;
    return [<span key={`t${i}`}>{before}</span>, <span key={`c${i}`} ref={node => { code.current[i] = node; }}>{run(fence.start, fence.end)}</span>];
  });
  return <>{parts}<span key="rest">{run(at, value.length)}</span>{" "}{fences.length > 0 && outlines.map((o, i) => <div key={i} className="cx-code" style={{ top: o.top, height: o.height }} />)}</>;
}

/// A pasted text's preview, above the field, or below it when there is no room above.
export function ComposePopup({ text, lines, node, expand, remove }: { text: string; lines: number; node(element: HTMLDivElement | null): void; expand(): void; remove(): void }) {
  const pop = useRef<HTMLDivElement>(null);
  const [down, setDown] = useState(false);
  useLayoutEffect(() => { if (!down && pop.current && pop.current.getBoundingClientRect().top < 4) setDown(true); }, [down]);
  return <div className={down ? "cx-pop down" : "cx-pop"} ref={element => { pop.current = element; node(element); }}><div className="h"><b>Pasted text</b><span>{plural(lines, "line", "lines")}</span><span className="sp" /><Button look="quiet" onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); expand(); }}>Expand</Button><Button look="quiet" onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); remove(); }}>Remove</Button></div><pre>{text}</pre></div>;
}
