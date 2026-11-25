import { Fragment, useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { flushSync } from "react-dom";
import type { MachineFolder } from "../project/projects-state.ts";
import type { FolderOffer } from "../project/addmachine-state.ts";
import { buildCapabilities, invoke } from "../ui/tauri.ts";
import { ensureMac } from "../machines/machine-store.ts";
import { openSheet } from "../ui/parts.tsx";
import { NO_MENU, completed, crumbs, isOpen, matching, menuKey, menuOf, splitTyped, suggest, type FolderMenu } from "./foldercomplete.ts";
import { Button, NO_AUTO, SkeletonRows, Spinner } from "../ui/kit.tsx";
import { Icon } from "../ui/icons.tsx";
import { reason } from "../ui/format.ts";
import { WINDOWS, commandKey } from "../ui/platform.ts";

interface PickerProps {
  /// The machine whose folders it shows.
  machine: string;
  /// Where it opens; null for where the machine starts.
  start: string | null;
  /// The line over the offers, such as "Add widget to laptop", and its verb for the key hints.
  heading: string; verb: string;
  /// The folder it points at where the one shown offers nothing.
  prefer: string;
  /// What to offer for the folder shown; `opened` is the first one it showed.
  offers(listing: MachineFolder, opened: string): FolderOffer[];
  /// What to say of the folder shown and of the first thing offered for it, such as how
  /// Syncthing lets go of it; null for nothing.
  noteFor(listing: MachineFolder, offer: FolderOffer | null): Promise<ReactNode>;
  /// Takes `offer`, made for the folder shown, `listing`.
  take(offer: FolderOffer, listing: MachineFolder): void;
  /// Esc in the empty field; with a filter it clears that first.
  close(): void;
  taking: boolean; error: string | null;
}

/// Keeps a path reading left to right in a cell that cuts its start, not its end.
const LTR = "\u200e";

/// A row of the picker: something to do with the folder shown, a path typed
/// to go to, or a folder in it to open.
type Row = { kind: "offer"; offer: FolderOffer } | { kind: "go"; path: string } | { kind: "folder"; name: string };

/// The folders each machine's pickers read, by machine and path, and each
/// machine's home: a folder read once, or read ahead with the one above it,
/// shows at once and is read again behind.
const known = new Map<string, Map<string, MachineFolder>>(), homes = new Map<string, string>();

/// The folder at `path` on `machine` as last read; null for home.
function cached(machine: string, path: string | null) {
  const home = homes.get(machine) ?? null;
  const full = path === null || path === "~" ? home : path.startsWith("~/") ? home && `${home}/${path.slice(2)}` : path;
  return full === null ? null : known.get(machine)?.get(full.replace(/(.)\/+$/, "$1")) ?? null;
}

/// Keeps `tree`, read at `path` on `machine`, with what was read before.
function remember(machine: string, path: string | null, tree: MachineFolder[]) {
  const folders = known.get(machine) ?? new Map<string, MachineFolder>();
  known.set(machine, folders);
  for (const listing of tree) folders.set(listing.path, listing);
  if ((path === null || path === "~") && tree[0]) homes.set(machine, tree[0].path);
}

/// A machine's folders as a palette: the path as steps back, a field that
/// filters the folder shown (or takes a path to go to), what the sheet offers
/// to do with it, then its folders to open. Up and Down move, Enter takes the
/// row, Cmd+Enter the first offer, Backspace in an empty field goes up, and
/// Esc clears the filter, then closes.
export function FolderPicker(p: PickerProps) {
  const [shown, setShown] = useState<{ listing: MachineFolder; opened: string } | null>(null);
  const [home, setHome] = useState<string | null>(null);
  const [filter, setFilter] = useState(""), [index, setIndex] = useState(0);
  const [reading, setReading] = useState(true), [failed, setFailed] = useState<string | null>(null);
  const [note, setNote] = useState<ReactNode>(null);
  const id = useId(), field = useRef<HTMLInputElement>(null), active = useRef<HTMLButtonElement>(null), ask = useRef(0);

  const show = (listing: MachineFolder) => setShown(was => ({ listing, opened: was?.opened ?? listing.path }));
  const go = (path: string | null) => {
    const mine = ++ask.current, hit = cached(p.machine, path);
    setReading(true);
    setFilter("");
    setFailed(null);
    if (hit) show(hit);

    invoke<MachineFolder[]>("list_machine_tree", { machineId: p.machine, path }).then(tree => {
      remember(p.machine, path, tree);
      if (mine !== ask.current) return;
      if (tree[0]) show(tree[0]);
      setReading(false);
    }, e => {
      if (mine !== ask.current) return;
      if (!hit) setFailed(reason(e));
      setReading(false);
    });
    field.current?.focus();
  };
  useEffect(() => {
    go(p.start);
    const home = homes.get(p.machine);
    if (home) setHome(home);
    else invoke<MachineFolder>("list_machine_folder", { machineId: p.machine, path: "~" }).then(listing => { homes.set(p.machine, listing.path); setHome(listing.path); }, () => {});

    return () => { ask.current++; };
  }, []);

  const listing = shown?.listing ?? null;
  const offered = shown ? p.offers(shown.listing, shown.opened) : [];
  const offeredPath = offered[0]?.path ?? null;
  useEffect(() => {
    setNote(null);
    if (!listing) return;

    let current = true;
    p.noteFor(listing, offered[0] ?? null).then(said => { if (current) setNote(said); }, () => {});
    return () => { current = false; };
  }, [listing?.path, offeredPath]);
  const typed = filter === "~" || splitTyped(filter) ? filter : null;
  const folders = listing && !typed ? matching(listing.folders, filter) : [];
  const rows: Row[] = [
    ...(filter ? [] : offered).map((offer): Row => ({ kind: "offer", offer })),
    ...(typed ? [{ kind: "go", path: typed } satisfies Row] : []),
    ...folders.map((name): Row => ({ kind: "folder", name })),
  ];
  const first = rows[0]?.kind === "folder" && !filter ? Math.max(0, folders.indexOf(p.prefer)) : 0;
  useLayoutEffect(() => { setIndex(first); }, [listing?.path, filter]);
  useLayoutEffect(() => { active.current?.scrollIntoView({ block: "nearest" }); }, [listing, index]);
  const at = rows[index] ?? null;

  const act = (row: Row | null) => {
    if (!row || !listing || p.taking) return;

    if (row.kind === "offer") p.take(row.offer, listing);
    else go(row.kind === "go" ? row.path : `${listing.path.replace(/\/$/, "")}/${row.name}`);
  };
  const key = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length) setIndex((index + (e.key === "ArrowDown" ? 1 : rows.length - 1)) % rows.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      act(commandKey(e) && offered.length ? { kind: "offer", offer: offered[0] } : at);
    } else if (e.key === "Backspace" && !filter && listing?.parent) {
      e.preventDefault();
      go(listing.parent);
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (filter) setFilter("");
      else p.close();
    }
  };

  const steps = listing ? crumbs(listing.path, home) : [];
  const hints = [
    at && ["↵", at.kind === "offer" ? p.verb : "Open"],
    at?.kind === "folder" && offered.length && [WINDOWS ? "Ctrl ↵" : "⌘↵", p.verb],
    rows.length > 1 && ["↑↓", "Navigate"],
    listing?.parent && ["⌫", "Up"],
    ["esc", filter ? "Clear" : "Close"],
  ].filter(hint => !!hint);
  return <><div className="q"><Icon name="find" />
    {steps.length > 0 && <div className="fp-path">{steps.map((step, i) => i < steps.length - 1
      ? <Button key={step.path} small tabIndex={-1} title={step.path} onClick={() => go(step.path)}>{step.label}</Button>
      : <b key={step.path} title={step.path}>{step.label}</b>)}</div>}
    <input ref={field} type="text" autoFocus placeholder="Filter, or type a path" {...NO_AUTO} aria-label={p.heading} role="combobox" aria-controls={id} aria-expanded="true" aria-activedescendant={at ? `${id}-${index}` : undefined} value={filter} onChange={e => setFilter(e.currentTarget.value)} onKeyDown={key} />
    {(reading || p.taking) && <Spinner />}</div>
    <div className="res" role="listbox" id={id}>
      {!listing && !failed && <SkeletonRows widths={[38, 52, 30]} />}
      {rows.map((row, i) => {
        const before = i ? rows[i - 1].kind : null;
        return <Fragment key={row.kind === "folder" ? `in/${row.name}` : row.kind === "go" ? "go" : `to/${row.offer.path}`}>
          {row.kind !== before && row.kind === "offer" && <h5>{p.heading}</h5>}
          {row.kind !== before && row.kind === "folder" && <h5>Folders<span>{folders.length + (filter ? 0 : listing?.more ?? 0)}</span></h5>}
          <button ref={i === index ? active : null} type="button" className="it" tabIndex={-1} role="option" id={`${id}-${i}`} aria-selected={i === index} disabled={p.taking} onMouseMove={e => { if (e.movementX || e.movementY) setIndex(i); }} onClick={() => act(row)}>
            {row.kind === "offer" ? <><Icon name="plus" /><b>{row.offer.label}</b><span className="w" title={row.offer.path}>{LTR}{row.offer.path}</span></>
              : row.kind === "go" ? <><Icon name="folder" size={14} /><span className="t"><span>Go to {row.path}</span></span><Icon name="chev" /></>
              : <><Icon name={listing?.repos.includes(row.name) ? "branch" : "folder"} size={14} /><span className="t"><span>{row.name}</span></span><Icon name="chev" /></>}
          </button>
        </Fragment>;
      })}
      {listing && !rows.length && <div className="none">{filter ? `No folder matches “${filter}”.` : "No folders in here."}</div>}
    </div>
    {(failed ?? p.error) ? <div className="am-msg bad">{failed ?? p.error}</div> : note && <div className="am-msg">{note}</div>}
    <div className="foot">{hints.map(([keys, does]) => <span key={keys}><kbd>{keys}</kbd>{does}</span>)}</div>
  </>;
}

/// A folder of `machine`, picked in the palette as a sheet of its own over
/// the one that asks; null when it is closed.
export function pickFolder(o: { machine: string; start: string | null; heading: string; noteFor?: PickerProps["noteFor"] }) {
  return new Promise<MachineFolder | null>(resolve => {
    const { close, render } = openSheet(o.heading, "cc-pal fp", { onClose: () => resolve(null) });
    flushSync(() => render(<FolderPicker machine={o.machine} start={o.start} heading={o.heading} verb="Use" prefer="" offers={listing => [{ label: "Use this folder", path: listing.path }]} noteFor={o.noteFor ?? (async () => null)} take={(_, listing) => { resolve(listing); close(); }} close={close} taking={false} error={null} />));
  });
}

/// A folder of this computer, picked in the palette; null when it is closed.
/// Refuses while Beep does not run on this computer, as its folders are read through it.
export async function pickHere(heading: string, start: string | null) {
  if (!(await buildCapabilities()).local_host) throw new Error("Folder browsing is not available on this device.");
  const mac = await ensureMac();
  if (!mac.id) throw new Error("Turn on “Let Beep use this computer” in Settings to choose its folders.");

  return (await pickFolder({ machine: mac.id, start, heading }))?.path ?? null;
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

  return <div className="ip-folder"><input ref={field} className="ip-path ip-dir" type="text" {...NO_AUTO} aria-label={p.label} role="combobox" aria-autocomplete="list" aria-controls={id} aria-expanded={opened} aria-activedescendant={opened ? `${id}-${menu.index}` : undefined} placeholder={p.placeholder} disabled={p.disabled} value={p.value} title={p.value} onChange={e => { p.change(e.currentTarget.value); look(e.currentTarget.value); }} onKeyDown={key} onBlur={() => { ask.current++; if (timer.current) clearTimeout(timer.current); setMenu(NO_MENU); }} />
    {opened && <div className="ip-menu" id={id} role="listbox">{menu.items.map((name, i) => <div key={name} ref={i === menu.index ? active : null} id={`${id}-${i}`} className={i === menu.index ? "on" : ""} role="option" aria-selected={i === menu.index} onMouseDown={e => e.preventDefault()} onClick={() => put(completed(menu.dir, name))}>{name}</div>)}</div>}</div>;
}
