import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { reason } from "./format.ts";
import type { ConfirmOptions, RenameOptions } from "./menu.ts";
import { Button, Checkbox, SheetHead } from "./kit.tsx";

/// The box of a right-click menu at `at`, kept inside the window. A click
/// outside it, leaving the window or a scroll closes it; Esc also gives focus back.
export function MenuBox({ at, close, escape, children }: { at: { x: number; y: number }; close(): void; escape(): void; children?: ReactNode }) {
  const menuBox = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<CSSProperties>({});
  useLayoutEffect(() => {
    const box = menuBox.current;
    if (!box) return;

    const fit = () => {
      const r = box.getBoundingClientRect();
      const left = Math.max(4, Math.min(at.x, innerWidth - r.width - 4)), top = Math.max(4, Math.min(at.y, innerHeight - r.height - 4));
      setPlace(was => was.left === left && was.top === top ? was : { left, top });
    };
    fit();

    // Lines can land after this pass, and a wider list must still fit.
    const resized = new ResizeObserver(fit);
    resized.observe(box);
    return () => resized.disconnect();
  }, [at, children]);
  useEffect(() => {
    const away = (event: MouseEvent) => { if (!(event.target instanceof Node) || !menuBox.current?.contains(event.target)) close(); };
    const keys = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      escape();
      event.preventDefault();
      event.stopImmediatePropagation();
    };
    window.addEventListener("mousedown", away, true);
    window.addEventListener("keydown", keys, true);
    window.addEventListener("blur", close);
    // Scrolling closes the menu, but not the scroll that brought the tapped
    // button into view, which lands in the next frame (the phone header).
    const frame = requestAnimationFrame(() => window.addEventListener("scroll", close, true));
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("mousedown", away, true);
      window.removeEventListener("keydown", keys, true);
      window.removeEventListener("blur", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [close, escape]);

  return <div className="cc-menu" ref={menuBox} style={place}>{children}</div>;
}

export function ConfirmView({ options, close, pending }: { options: ConfirmOptions; close(): void; pending(value: boolean): void }) {
  const [busy, setBusy] = useState(false), [ticked, setTicked] = useState(false);
  const [error, setError] = useState<string | null>(null), [result, setResult] = useState<string | null>(null);
  const action = useRef<HTMLButtonElement>(null), currentTick = useRef(false);
  useLayoutEffect(() => { action.current?.focus(); }, []);
  const run = async () => {
    if (busy) return;
    pending(true); setBusy(true); setError(null);
    try {
      const response = await options.run(ticked);
      pending(false); setBusy(false);
      if (currentTick.current && typeof response === "string" && response) setResult(response);
      else close();
    } catch (failure) {
      pending(false); setBusy(false); setError(reason(failure));
    }
  };
  if (result !== null) return <><SheetHead title={options.title} /><div className="am-msg">{result}</div><div className="gp-foot"><Button onClick={close}>Close</Button></div></>;
  const note = ticked && options.tick ? options.tick.note : options.note;
  return <><SheetHead title={options.title} /><div className="am-msg">{options.text}</div>{options.tick && <label className="am-sub am-tick"><Checkbox checked={ticked} change={checked => { currentTick.current = checked; setTicked(checked); }} />{options.tick.label}</label>}<div className="am-sub" hidden={!note}>{note}</div><div className="am-msg bad" hidden={error === null}>{error}</div><div className="gp-foot"><Button look="quiet" onClick={close}>Cancel</Button><Button ref={action} danger disabled={busy} onClick={() => { void run(); }}>{options.action}</Button></div></>;
}

export function RenameView({ options, close, pending }: { options: RenameOptions; close(): void; pending(value: boolean): void }) {
  const [text, setText] = useState(options.value), [busy, setBusy] = useState(false), [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { field.current?.focus(); field.current?.select(); }, []);
  const save = async () => {
    const value = text.trim();
    if (busy) return;
    if (!value || value === options.value) return close();
    pending(true); setBusy(true); setError(null);
    try {
      await options.run(value);
      pending(false); setBusy(false); close();
    } catch (failure) {
      pending(false); setBusy(false); setError(reason(failure));
    }
  };
  return <><SheetHead title={options.title} /><div className="am-line"><input ref={field} className="gp-field" value={text} spellCheck={false} onChange={event => setText(event.currentTarget.value)} onKeyDown={event => { if (event.key === "Enter" && !event.nativeEvent.isComposing) { event.preventDefault(); void save(); } }} /></div><div className="am-msg bad" hidden={error === null}>{error}</div><div className="gp-foot"><Button look="quiet" onClick={close}>Cancel</Button><Button look="primary" disabled={busy} onClick={() => { void save(); }}>Save</Button></div></>;
}

