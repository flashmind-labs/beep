import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { NO_AUTO } from "../../ui/kit.tsx";

/// The session's title, renamed with a double-click unless `save` is null.
export function SessionTitle({ title, save }: { title: string; save: ((value: string) => Promise<void>) | null }) {
  const [draft, setDraft] = useState<string | null>(null), [saving, setSaving] = useState(false), [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null), button = useRef<HTMLElement>(null), busy = useRef(false), focusBack = useRef(false), mounted = useRef(true);
  useEffect(() => () => { mounted.current = false; }, []);
  useLayoutEffect(() => {
    if (draft !== null && input.current && document.activeElement !== input.current) { input.current.focus(); input.current.select(); }
    else if (draft === null && focusBack.current) { focusBack.current = false; button.current?.focus(); }
  }, [draft === null]);
  const close = (focus: boolean) => { focusBack.current = focus; setError(null); setDraft(null); };
  const commit = async (focus: boolean) => {
    if (draft === null || busy.current) return;
    const value = draft.trim();
    if (value === title) { close(focus); return; }
    busy.current = true; setSaving(true); setError(null);
    try { await save?.(value); if (mounted.current) close(focus); }
    catch (failure) { if (mounted.current) { setError(`Could not save title: ${failure instanceof Error ? failure.message : String(failure)}`); input.current?.focus(); } }
    finally { busy.current = false; if (mounted.current) setSaving(false); }
  };
  const begin = () => { if (draft === null) setDraft(title); };
  if (!save) return <b id="agtitle" title={title}>{title}</b>;
  return <><b ref={button} id="agtitle" tabIndex={0} role="button" aria-label={`Rename session: ${title}`} aria-busy={saving || undefined} title={`${title}\nDouble-click to rename (Enter)`} onDoubleClick={begin} onKeyDown={event => { if (event.target === event.currentTarget && event.key === "Enter") { event.preventDefault(); event.stopPropagation(); begin(); } }}>{draft === null ? title : <input ref={input} className="agtitle-input" value={draft} maxLength={200} {...NO_AUTO} aria-label="Session title" title="Enter to save, Escape to cancel. Clear to restore the agent title." readOnly={saving} aria-invalid={error ? true : undefined} onChange={event => setDraft(event.currentTarget.value)} onBlur={() => { void commit(false); }} onKeyDown={event => { event.stopPropagation(); if (event.nativeEvent.isComposing) return; if (event.key === "Escape") { event.preventDefault(); if (!busy.current) close(true); } else if (event.key === "Enter") { event.preventDefault(); void commit(true); } }} />}</b>{error && <small className="agtitle-error" role="alert">{error}</small>}</>;
}
