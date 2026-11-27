import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { useInteractOutside } from "react-aria/useInteractOutside";
import type { StashEntry } from "../../stores/chat/stash-state.ts";
import { useDialogRef } from "../../ui/parts.tsx";

interface StashPanelProps {
  project: string;
  entries: StashEntry[];
  busy: boolean;
  canPop: boolean;
  swap: boolean;
  /// The Stash button it opened from, which it sits above.
  anchor: HTMLElement;
  onClose(): void;
  onRemove(id: string): void;
  onPop(id: string): void;
  onOutside(target: EventTarget | null): void;
}

export function StashPanel({ project, entries, busy, canPop, swap, anchor, onClose, onRemove, onPop, onOutside }: StashPanelProps) {
  const panel = useRef<HTMLDivElement>(null), dialog = useDialogRef(panel);
  const place = usePlace(panel, anchor, onClose);
  useInteractOutside({ ref: panel, onInteractOutside: e => onOutside(e.target) });

  return <div ref={dialog} className="prompt-stash-panel" role="dialog" aria-label="Project stash" style={place}><header><div><b>Stash</b><small>{project}</small></div><button type="button" data-stash-close aria-label="Close stash" autoFocus onClick={onClose}>×</button></header><div className="prompt-stash-items">{entries.map(item => <article key={item.id}><p>{item.text || "Attachments only"}</p><div className="prompt-stash-files">{[...item.images, ...item.files].map((name, index) => <span key={index}>{name}</span>)}</div><footer><button type="button" data-stash-remove={item.id} disabled={busy} onClick={() => onRemove(item.id)}>Remove</button><button type="button" data-stash-pop={item.id} disabled={busy || !canPop} onClick={() => onPop(item.id)}>{swap ? "Stash draft & pop" : "Pop"}</button></footer></article>)}</div></div>;
}

/// Where the panel sits: above its button, right edges aligned, inside the window. It
/// follows the button as the window resizes or the page scrolls, and closes with it.
function usePlace(panel: React.RefObject<HTMLDivElement | null>, anchor: HTMLElement, close: () => void) {
  const [place, setPlace] = useState<CSSProperties>({});

  useLayoutEffect(() => {
    const measure = () => {
      if (!anchor.isConnected) { close(); return; }

      const rect = anchor.getBoundingClientRect(), width = Math.min(400, innerWidth - 32);
      setPlace({ width, left: Math.max(16, Math.min(rect.right - width, innerWidth - width - 16)), bottom: Math.max(16, innerHeight - rect.top + 8), maxHeight: Math.max(100, rect.top - 24) });
    };
    const scrolled = (e: Event) => { if (!(e.target instanceof Node && panel.current?.contains(e.target))) measure(); };

    measure();
    addEventListener("resize", measure);
    document.addEventListener("scroll", scrolled, true);
    return () => { removeEventListener("resize", measure); document.removeEventListener("scroll", scrolled, true); };
  }, [anchor, close, panel]);

  return place;
}
