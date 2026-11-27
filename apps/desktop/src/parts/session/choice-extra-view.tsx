import { useLayoutEffect, useRef, type KeyboardEvent, type RefObject } from "react";
import { NO_AUTO } from "../../ui/kit.tsx";
import { Icon } from "../../ui/icons.tsx";
import { isOpen, type FolderMenu } from "../../stores/addproject/foldercomplete.ts";

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
  return <span className="agc-type"><input ref={input} id="agcwd" {...NO_AUTO} role="combobox" aria-label="Folder" aria-expanded={isOpen(menu)} placeholder="/path/to/folder or ~/folder" value={value} onChange={event => change(event.currentTarget.value)} onKeyDown={keyDown} onBlur={blur} /><span id="agcwdmenu">{isOpen(menu) && <div className="agc-menu" role="listbox">{menu.items.map((name, index) => <button type="button" key={name} data-agfi={index} className={index === menu.index ? "on" : ""} role="option" onMouseDown={event => event.preventDefault()} onClick={event => { event.stopPropagation(); pick(index); }}><span /><span className="mono">{name}</span><small /></button>)}</div>}</span></span>;
}

export function BaseInput({ value, placeholder, change, keyDown, leave }: TypedField & { placeholder: string }) {
  const { input, blur } = useTyped(leave);
  return <span className="agc-type base"><input ref={input} id="agbase" {...NO_AUTO} aria-label="Base branch" placeholder={placeholder} value={value} onChange={event => change(event.currentTarget.value)} onKeyDown={keyDown} onBlur={blur} /></span>;
}

export function BaseChoice({ base, open, button, toggle }: { base: string; open: boolean; button: RefObject<HTMLButtonElement | null>; toggle(): void }) {
  return <button ref={button} type="button" className="pill" id="agbaseb" aria-expanded={open} title="The branch the new one starts from" onClick={toggle}><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="7" r="2" /><path d="M6 7v10M18 9c0 5-6 4-11 8" /></svg><small>from</small> <span className="mono">{base}</span><Icon name="caretDown" size={12} /></button>;
}

/// The named account the agent signs in as, when it has any.
export function AccountChoice({ account, open, button, toggle }: { account: string | null; open: boolean; button: RefObject<HTMLButtonElement | null>; toggle(): void }) {
  return <button ref={button} type="button" className="pill" id="agacctb" aria-expanded={open} title="The account the agent signs in as" onClick={toggle}><small>as</small> <span>{account ?? "default"}</span><Icon name="caretDown" size={12} /></button>;
}
