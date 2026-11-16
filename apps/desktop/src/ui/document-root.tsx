import { Fragment, memo, useSyncExternalStore, type ReactNode } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Guarded } from "./kit.tsx";

interface Entry<Slot extends string> { slot: Slot; host: HTMLElement | null; content: ReactNode; identity?: string }
interface Scope { id: number; render(): ReactNode; subscribe(listener: () => void): () => void; snapshot(): number }
const scopes = new Map<number, Scope>();
let next = 0, root: Root | null = null;
/// Set when a window's own tree renders the scopes in place of the shared root.
let hosted: { revision: number; listeners: Set<() => void> } | null = null;

const Scoped = memo(function Scoped({ scope }: { scope: Scope }) {
  useSyncExternalStore(scope.subscribe, scope.snapshot, scope.snapshot);
  return <Guarded>{scope.render()}</Guarded>;
});

function renderScopes() {
  if (hosted) {
    const host = hosted;
    host.revision++;
    flushSync(() => { for (const listener of host.listeners) listener(); });
    return;
  }

  root ??= createRoot(document.createElement("div"));
  const owner = root;
  flushSync(() => owner.render(<>{[...scopes.values()].map(scope => <Scoped key={scope.id} scope={scope} />)}</>));
}

/// The scopes as part of a window's own tree, so the window has one root. Call it
/// before any scope mounts, once, from the window's root render.
export function documentScopes(): ReactNode {
  if (root) throw new Error("Document scopes already have their own root");
  const host = hosted ??= { revision: 0, listeners: new Set() };
  const subscribe = (listener: () => void) => { host.listeners.add(listener); return () => { host.listeners.delete(listener); }; };
  const snapshot = () => host.revision;
  function Scopes() {
    useSyncExternalStore(subscribe, snapshot, snapshot);
    return [...scopes.values()].map(scope => <Scoped key={scope.id} scope={scope} />);
  }
  // Content without a host stays out of the page, as it did in the shared root.
  return createPortal(<Scopes />, document.createElement("div"));
}

/// A scope owns its portals; updates notify only this scope's subscribers.
export function createDocumentScope<Slot extends string>(wrap: (children: ReactNode) => ReactNode = children => children) {
  const entries = new Map<Slot, Entry<Slot>>();
  const listeners = new Set<() => void>();
  let revision = 0, mounted = false, disposed = false;
  const scope: Scope = {
    id: ++next,
    subscribe: listener => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    snapshot: () => revision,
    render: () => wrap(<>{[...entries.values()].map(entry => entry.host
      ? createPortal(entry.content, entry.host, `${entry.slot}:${entry.identity ?? entry.slot}`)
      : <Fragment key={`${entry.slot}:${entry.identity ?? entry.slot}`}>{entry.content}</Fragment>)}</>),
  };
  const mount = () => {
    if (mounted || disposed) return;
    mounted = true;
    scopes.set(scope.id, scope);
    renderScopes();
  };
  const changed = () => { revision++; for (const listener of listeners) listener(); };
  const setDeferred = (updates: readonly Entry<Slot>[]) => {
    if (disposed) return;
    let dirty = false;
    for (const entry of updates) {
      const before = entries.get(entry.slot);
      if (before && before.host === entry.host && before.content === entry.content && before.identity === entry.identity) continue;
      entries.set(entry.slot, entry);
      dirty = true;
    }
    if (dirty) changed();
  };
  const set = (updates: readonly Entry<Slot>[]) => { if (disposed) return; mount(); flushSync(() => setDeferred(updates)); };
  const remove = (slots: readonly Slot[]) => {
    if (disposed) return;
    let dirty = false;
    for (const slot of slots) dirty = entries.delete(slot) || dirty;
    if (dirty) flushSync(changed);
  };
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    if (!mounted) return;
    mounted = false;
    entries.clear();
    scopes.delete(scope.id);
    renderScopes();
  };
  return { mount, set, setDeferred, remove, dispose };
}

/// One portal view with its caller's existing flush boundary.
export function createDocumentView(host: HTMLElement) {
  const scope = createDocumentScope<"view">();
  scope.mount();
  let started = false, disposed = false;
  return {
    render: (content: ReactNode) => {
      if (disposed) return;
      // Match root ownership when taking over a native loading placeholder.
      if (!started) { host.replaceChildren(); started = true; }
      scope.setDeferred([{ slot: "view", host, content }]);
    },
    unmount: () => { disposed = true; scope.dispose(); },
  };
}

export type DocumentView = ReturnType<typeof createDocumentView>;
