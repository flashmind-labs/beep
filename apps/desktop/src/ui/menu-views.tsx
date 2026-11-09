import { useLayoutEffect, useRef, useState } from "react";
import { Menu, MenuItem } from "react-aria-components";
import { reason } from "./format.ts";
import type { ConfirmOptions, MenuItem as Action, RenameOptions } from "./menu.ts";

export function ContextMenu({ title, items, choose }: { title: string; items: readonly Action[]; choose(index: number): void }) {
  return <><span className="lbl">{title}</span><Menu aria-label={title} onAction={key => choose(Number(key))}>{items.map((item, index) => <MenuItem id={String(index)} key={index} textValue={item.label} className={item.warn ? "warn" : ""}>{item.label}</MenuItem>)}</Menu></>;
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
  if (result !== null) return <><Heading title={options.title} /><div className="am-msg">{result}</div><div className="gp-foot"><button type="button" className="dbtn" onClick={close}>Close</button></div></>;
  const note = ticked && options.tick ? options.tick.note : options.note;
  return <><Heading title={options.title} /><div className="am-msg">{options.text}</div>{options.tick && <label className="am-sub am-tick"><input type="checkbox" checked={ticked} onChange={event => { currentTick.current = event.currentTarget.checked; setTicked(currentTick.current); }} />{options.tick.label}</label>}<div className="am-sub" hidden={!note}>{note}</div><div className="am-msg bad" hidden={error === null}>{error}</div><div className="gp-foot"><button type="button" className="dbtn quiet" onClick={close}>Cancel</button><button ref={action} type="button" className="dbtn warn" disabled={busy} onClick={() => { void run(); }}>{options.action}</button></div></>;
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
  return <><Heading title={options.title} /><div className="am-line"><input ref={field} className="gp-field" value={text} spellCheck={false} onChange={event => setText(event.currentTarget.value)} onKeyDown={event => { if (event.key === "Enter" && !event.nativeEvent.isComposing) { event.preventDefault(); void save(); } }} /></div><div className="am-msg bad" hidden={error === null}>{error}</div><div className="gp-foot"><button type="button" className="dbtn quiet" onClick={close}>Cancel</button><button type="button" className="dbtn pri" disabled={busy} onClick={() => { void save(); }}>Save</button></div></>;
}

function Heading({ title }: { title: string }) { return <div className="am-head"><b>{title}</b><kbd>esc</kbd></div>; }
