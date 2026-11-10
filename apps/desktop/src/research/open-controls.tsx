import { useLayoutEffect, useRef } from "react";

export function ResearchNotice({ text, path, bad, busy, reveal }: { text: string; path: string | null; bad: boolean; busy: boolean; reveal(path: string): void }) {
  return <div className={`dsaved${bad ? " bad" : ""}`} role="status">{busy && <span className="dspin" />}<span>{text}</span>{path && <button className="dbtn" onClick={() => reveal(path)}>Show in Finder</button>}</div>;
}

export function ResearchQuote({ rect, quote }: { rect: DOMRect; quote(): void }) {
  const button = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => {
    const el = button.current;
    if (!el) return;
    const width = el.offsetWidth, height = el.offsetHeight;
    el.style.left = `${Math.max(8, Math.min(innerWidth - width - 8, rect.left + rect.width / 2 - width / 2))}px`;
    el.style.top = `${rect.top - height - 8 < 46 ? rect.bottom + 8 : rect.top - height - 8}px`;
  }, [rect]);
  return <button ref={button} id="rquote" className="dbtn pri rquote" onMouseDown={event => event.preventDefault()} onClick={quote}>Quote<kbd>⌘'</kbd></button>;
}
