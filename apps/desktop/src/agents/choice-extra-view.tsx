import { useEffect, useRef } from "react";
import { isOpen, type FolderMenu } from "../addproject/foldercomplete.ts";

export function FolderInput({ value, menu, bind, pick }: { value: string; menu: FolderMenu; bind(input: HTMLInputElement): void; pick(index: number): void }) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { queueMicrotask(() => { if (input.current?.isConnected) bind(input.current); }); }, []);
  return <span className="agc-type"><input ref={input} id="agcwd" spellCheck={false} autoComplete="off" role="combobox" aria-label="Folder" aria-expanded={isOpen(menu)} placeholder="/path/to/folder or ~/folder" defaultValue={value} /><span id="agcwdmenu">{isOpen(menu) && <div className="agc-menu" role="listbox">{menu.items.map((name, index) => <button type="button" key={name} data-agfi={index} className={index === menu.index ? "on" : ""} role="option" onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); pick(index); }}><span /><span className="mono">{name}</span><small /></button>)}</div>}</span></span>;
}

export function BaseInput({ value, placeholder, bind }: { value: string; placeholder: string; bind(input: HTMLInputElement): void }) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { queueMicrotask(() => { if (input.current?.isConnected) bind(input.current); }); }, []);
  return <span className="agc-type base"><input ref={input} id="agbase" spellCheck={false} autoComplete="off" aria-label="Base branch" placeholder={placeholder} defaultValue={value} /></span>;
}

export function BaseChoice({ base, open }: { base: string; open: boolean }) {
  return <button type="button" className="pill" id="agbaseb" aria-expanded={open} title="The branch the new one starts from"><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="7" r="2" /><path d="M6 7v10M18 9c0 5-6 4-11 8" /></svg><small>from</small> <span className="mono">{base}</span><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6" /></svg></button>;
}
