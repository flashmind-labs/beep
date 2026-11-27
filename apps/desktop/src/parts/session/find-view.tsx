import type { RefObject } from "react";
import { NO_AUTO } from "../../ui/kit.tsx";
import { Icon } from "../../ui/icons.tsx";

interface FindViewProps {
  field: RefObject<HTMLInputElement | null>;
  error: string;
  status: string;
  regex: boolean;
  disabled: boolean;
  input(): void;
  enter(reverse: boolean): void;
  toggleRegex(): void;
  move(step: number): void;
  close(): void;
}

export function FindView(p: FindViewProps) {
  return <div className={`chat-find${p.error ? " invalid" : ""}`} role="search" aria-label="Search conversation" onKeyDown={event => {
    if (event.nativeEvent.isComposing || event.key !== "Enter" || !(event.target instanceof HTMLInputElement)) return;
    event.preventDefault(); event.stopPropagation(); p.enter(event.shiftKey);
  }}><input ref={p.field} placeholder="Find in chat" aria-label="Find in chat" aria-describedby="chat-find-status" {...NO_AUTO} aria-invalid={p.error ? true : undefined} onInput={p.input} /><button type="button" className="chat-find-regex" title="Use regular expression" aria-label="Use regular expression" aria-pressed={p.regex} onClick={p.toggleRegex}>.*</button><output id="chat-find-status" aria-live="polite" title={p.error || undefined}>{p.status}</output><button type="button" aria-label="Previous match (Shift+Enter)" title="Previous match (Shift+Enter)" disabled={p.disabled} onClick={() => p.move(-1)}><Icon name="caretUp" /></button><button type="button" aria-label="Next match (Enter)" title="Next match (Enter)" disabled={p.disabled} onClick={() => p.move(1)}><Icon name="caretDown" /></button><button type="button" aria-label="Close search (Esc)" title="Close search (Esc)" onClick={p.close}><Icon name="cross" /></button></div>;
}
