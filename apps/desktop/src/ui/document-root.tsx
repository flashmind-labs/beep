import { Fragment, memo, useSyncExternalStore, type ReactNode } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";

interface Entry<Slot extends string> { slot: Slot; host: HTMLElement | null; content: ReactNode; identity?: string }
interface Scope { id: number; render(): ReactNode; subscribe(listener: () => void): () => void; snapshot(): number }
const scopes = new Map<number, Scope>();
let next = 0, root: Root | null = null;

const Scoped = memo(function Scoped({ scope }: { scope: Scope }) {
  useSyncExternalStore(scope.subscribe, scope.snapshot, scope.snapshot);
  return scope.render();
});

function renderScopes() {
  root ??= createRoot(document.createElement("div"));
  const owner = root;
  flushSync(() => owner.render(<>{[...scopes.values()].map(scope => <Scoped key={scope.id} scope={scope} />)}</>));
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
    for (const entry of updates) entries.set(entry.slot, entry);
    changed();
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

/// Existing callers can keep their flushSync boundary while moving off createRoot.
export function createDocumentView(host: HTMLElement) {
  const scope = createDocumentScope<"view">();
  scope.mount();
  return {
    render: (content: ReactNode) => scope.setDeferred([{ slot: "view", host, content }]),
    unmount: () => scope.dispose(),
  };
}

export type DocumentView = ReturnType<typeof createDocumentView>;
