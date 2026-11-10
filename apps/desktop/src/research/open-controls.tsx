import { useLayoutEffect, useRef, useState } from "react";

export function ResearchNotice({ text, path, bad, busy, reveal }: { text: string; path: string | null; bad: boolean; busy: boolean; reveal(path: string): void }) {
  return <div className={`dsaved${bad ? " bad" : ""}`} role="status">{busy && <span className="dspin" />}<span>{text}</span>{path && <button className="dbtn" onClick={() => reveal(path)}>Show in Finder</button>}</div>;
}

/// The Quote button over the selection, above it unless that leaves the window's top.
export function ResearchQuote({ rect, quote }: { rect: DOMRect; quote(): void }) {
  const button = useRef<HTMLButtonElement>(null);
  const [place, setPlace] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    const el = button.current;
    if (!el) return;
    const width = el.offsetWidth, height = el.offsetHeight;
    setPlace({ left: Math.max(8, Math.min(innerWidth - width - 8, rect.left + rect.width / 2 - width / 2)), top: rect.top - height - 8 < 46 ? rect.bottom + 8 : rect.top - height - 8 });
  }, [rect]);

  return <button ref={button} id="rquote" className="dbtn pri rquote" style={place ?? undefined} onMouseDown={event => event.preventDefault()} onClick={quote}>Quote<kbd>⌘'</kbd></button>;
}
