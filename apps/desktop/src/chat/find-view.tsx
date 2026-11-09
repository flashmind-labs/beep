interface FindViewProps {
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

function Icon({ path }: { path: string }) {
  return <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d={path} /></svg>;
}

export function FindView(p: FindViewProps) {
  return <div className={`chat-find${p.error ? " invalid" : ""}`} role="search" aria-label="Search conversation" onKeyDown={event => {
    if (event.nativeEvent.isComposing || event.key !== "Enter" || !(event.target instanceof HTMLInputElement)) return;
    event.preventDefault(); event.stopPropagation(); p.enter(event.shiftKey);
  }}><input placeholder="Find in chat" aria-label="Find in chat" aria-describedby="chat-find-status" autoComplete="off" spellCheck={false} aria-invalid={p.error ? true : undefined} onInput={p.input} /><button type="button" className="chat-find-regex" title="Use regular expression" aria-label="Use regular expression" aria-pressed={p.regex} onClick={p.toggleRegex}>.*</button><output id="chat-find-status" aria-live="polite" title={p.error || undefined}>{p.status}</output><button type="button" aria-label="Previous match (Shift+Enter)" title="Previous match (Shift+Enter)" disabled={p.disabled} onClick={() => p.move(-1)}><Icon path="m18 15-6-6-6 6" /></button><button type="button" aria-label="Next match (Enter)" title="Next match (Enter)" disabled={p.disabled} onClick={() => p.move(1)}><Icon path="m6 9 6 6 6-6" /></button><button type="button" aria-label="Close search (Esc)" title="Close search (Esc)" onClick={p.close}><Icon path="m6 6 12 12M6 18 18 6" /></button></div>;
}
