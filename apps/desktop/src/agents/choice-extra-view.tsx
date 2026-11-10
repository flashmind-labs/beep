import { useLayoutEffect, useRef, type KeyboardEvent, type RefObject } from "react";
import { isOpen, type FolderMenu } from "../addproject/foldercomplete.ts";

interface TypedField {
  value: string;
  change(value: string): void;
  keyDown(event: KeyboardEvent<HTMLInputElement>): void;
  /// Focus left the field for good.
  leave(): void;
}

/// Focused on mount with the caret at the end; leaving it, unless focus comes back at once, finishes it.
function useTyped(leave: () => void) {
  const input = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    const field = input.current;
    field?.focus();
    field?.setSelectionRange(field.value.length, field.value.length);
  }, []);
  const blur = () => setTimeout(() => { if (input.current && document.activeElement !== input.current) leave(); }, 150);
  return { input, blur };
}

export function FolderInput({ value, menu, change, keyDown, leave, pick }: TypedField & { menu: FolderMenu; pick(index: number): void }) {
  const { input, blur } = useTyped(leave);
  return <span className="agc-type"><input ref={input} id="agcwd" spellCheck={false} autoComplete="off" role="combobox" aria-label="Folder" aria-expanded={isOpen(menu)} placeholder="/path/to/folder or ~/folder" value={value} onChange={event => change(event.currentTarget.value)} onKeyDown={keyDown} onBlur={blur} /><span id="agcwdmenu">{isOpen(menu) && <div className="agc-menu" role="listbox">{menu.items.map((name, index) => <button type="button" key={name} data-agfi={index} className={index === menu.index ? "on" : ""} role="option" onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); pick(index); }}><span /><span className="mono">{name}</span><small /></button>)}</div>}</span></span>;
}

export function BaseInput({ value, placeholder, change, keyDown, leave }: TypedField & { placeholder: string }) {
  const { input, blur } = useTyped(leave);
  return <span className="agc-type base"><input ref={input} id="agbase" spellCheck={false} autoComplete="off" aria-label="Base branch" placeholder={placeholder} value={value} onChange={event => change(event.currentTarget.value)} onKeyDown={keyDown} onBlur={blur} /></span>;
}

export function BaseChoice({ base, open, button, toggle }: { base: string; open: boolean; button: RefObject<HTMLButtonElement | null>; toggle(): void }) {
  return <button ref={button} type="button" className="pill" id="agbaseb" aria-expanded={open} title="The branch the new one starts from" onClick={toggle}><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="7" r="2" /><path d="M6 7v10M18 9c0 5-6 4-11 8" /></svg><small>from</small> <span className="mono">{base}</span><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M6 9l6 6 6-6" /></svg></button>;
}
