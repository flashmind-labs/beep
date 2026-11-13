import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import type { MachineFolder } from "../project/projects-state.ts";
import { invoke } from "../ui/tauri.ts";
import { NO_MENU, completed, isOpen, menuKey, menuOf, splitTyped, suggest, type FolderMenu } from "./foldercomplete.ts";
import { Button } from "../ui/kit.tsx";

interface Props {
  listing: MachineFolder; reading: boolean; note: string | null; focusFirst: boolean;
  browse(path: string | null): void; cancel(): void; use(listing: MachineFolder): void;
}

/// Shared remote folder controls; each sheet owns its navigation and selection.
export function FolderBrowser(p: Props) {
  const { listing } = p, first = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { if (p.focusFirst) first.current?.focus(); }, [listing.path, p.focusFirst]);
  const into = (name: string) => `${listing.path.replace(/\/+$/, "")}/${name}`;
  return <div className="ip-browse"><div className="at"><Button look="quiet" className="ip-up" disabled={!listing.parent || p.reading} onClick={() => p.browse(listing.parent)}>Up</Button><span title={listing.path}>{listing.path}</span></div>
    <div className="dirs">{listing.folders.map((name, index) => <button type="button" ref={index === 0 ? first : null} key={name} disabled={p.reading} onClick={() => p.browse(into(name))}>{name}</button>)}{!listing.folders.length && <span className="none">{listing.exists ? "No folders in here" : "Not there yet"}</span>}{!!listing.more && <span className="none">and {listing.more} more</span>}</div>
    <div className="foot"><span className="note">{p.note}</span><Button look="quiet" onClick={p.cancel}>Cancel</Button><Button look="primary" onClick={() => p.use(listing)}>Use this folder</Button></div>
  </div>;
}

interface FieldProps {
  /// The machine whose folders the menu offers; null offers none.
  machine: string | null; value: string; label: string; placeholder: string; disabled: boolean;
  change(value: string): void;
  /// Enter with no menu open.
  enter(): void;
}

/// A folder path field that offers the machine's folders as it is typed:
/// Up and Down move, Tab or Enter take one, Esc closes the menu.
export function FolderField(p: FieldProps) {
  const [menu, setMenu] = useState<FolderMenu>(NO_MENU);
  const id = useId(), field = useRef<HTMLInputElement>(null), active = useRef<HTMLDivElement>(null);
  const ask = useRef(0), timer = useRef<ReturnType<typeof setTimeout> | null>(null), listed = useRef(new Map<string, readonly string[]>());
  const opened = isOpen(menu);
  useLayoutEffect(() => { if (opened) active.current?.scrollIntoView({ block: "nearest" }); }, [opened, menu.index, menu.dir]);
  useEffect(() => () => { ask.current++; if (timer.current) clearTimeout(timer.current); }, []);

  const offer = (next: FolderMenu) => setMenu(document.activeElement === field.current ? next : NO_MENU);
  const look = (text: string) => {
    if (timer.current) clearTimeout(timer.current);
    const mine = ++ask.current, machine = p.machine, typed = splitTyped(text);
    if (!machine || !typed) { setMenu(NO_MENU); return; }
    const key = `${machine}\n${typed.dir}`, known = listed.current.get(key);
    const show = (folders: readonly string[]) => { if (mine === ask.current) offer(menuOf(typed.dir, suggest(folders, typed.prefix))); };
    if (known) { show(known); return; }
    timer.current = setTimeout(() => {
      invoke<MachineFolder>("list_machine_folder", { machineId: machine, path: typed.dir }).then(listing => {
        listed.current.set(key, listing.folders);
        show(listing.folders);
      }, () => { if (mine === ask.current) setMenu(NO_MENU); });
    }, 150);
  };
  const put = (text: string) => {
    p.change(text);
    look(text);
    requestAnimationFrame(() => field.current?.setSelectionRange(text.length, text.length));
  };
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    const act = menuKey(menu, e.key, e.shiftKey || e.altKey || e.ctrlKey || e.metaKey);
    if (act.type === "none") {
      if (e.key === "Enter") { e.preventDefault(); p.enter(); }
      return;
    }
    e.preventDefault();
    if (act.type === "move") setMenu({ ...menu, index: act.index });
    else if (act.type === "close") setMenu(NO_MENU);
    else put(act.text);
  };

  return <div className="ip-folder"><input ref={field} className="ip-path ip-dir" type="text" spellCheck={false} autoCapitalize="off" autoCorrect="off" aria-label={p.label} role="combobox" aria-autocomplete="list" aria-controls={id} aria-expanded={opened} aria-activedescendant={opened ? `${id}-${menu.index}` : undefined} placeholder={p.placeholder} disabled={p.disabled} value={p.value} title={p.value} onChange={e => { p.change(e.currentTarget.value); look(e.currentTarget.value); }} onKeyDown={key} onBlur={() => { ask.current++; if (timer.current) clearTimeout(timer.current); setMenu(NO_MENU); }} />
    {opened && <div className="ip-menu" id={id} role="listbox">{menu.items.map((name, i) => <div key={name} ref={i === menu.index ? active : null} id={`${id}-${i}`} className={i === menu.index ? "on" : ""} role="option" aria-selected={i === menu.index} onMouseDown={e => e.preventDefault()} onClick={() => put(completed(menu.dir, name))}>{name}</div>)}</div>}</div>;
}
