import type { HintViewModel } from "../bar/hints-view.tsx";
import { createRef, type ReactNode } from "react";
import { createStore, type StoreApi } from "zustand";
import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-page.ts";
import type { ChatBody } from "../ui/turn.ts";

export interface AgentHintsModel { value: HintViewModel; project: string | null }
export interface Frame { head: ReactNode; field: ReactNode; body: ChatBody; hints: AgentHintsModel }
/// What the session page draws: the frame, and the views put in it.
export interface FrameState { frame: Frame; revision: number; slots: readonly Slot[] }
export interface Owner { page: BarPageSlot; store: StoreApi<FrameState>; revision: number; frame: Frame; portals: Map<HTMLElement, Slot>; children: Map<string, Slot>; next: number; release(): void }
let current: Owner | null = null;

/// The session page's owner while a frame shows.
export const frameOwner = () => current;

// Each child and portal is its own store, so a pane redrawing one tab strip
// renders that strip alone instead of the whole session frame.
export interface Slot { key: number; host: HTMLElement | null; view: StoreApi<ReactNode> }

function slot(owner: Owner, host: HTMLElement | null, view: ReactNode): Slot {
  return { key: ++owner.next, host, view: createStore<ReactNode>(() => view) };
}

export function draw(owner: Owner) {
  // Something replaced the bar's contents: the page box mounts again.
  if (!owner.page.host.isConnected) owner.page.show({ view: "session", props: { store: owner.store } });
  flushSync(() => owner.store.setState({ frame: owner.frame, revision: owner.revision, slots: [...owner.children.values(), ...owner.portals.values()] }));
}


function update(entry: Slot, view: ReactNode) {
  if (entry.view.getState() === view) return;
  flushSync(() => entry.view.setState(view, true));
}

export function setSessionChild(key: string, view: ReactNode) {
  const owner = current;
  if (!owner) return;
  const prior = owner.children.get(key);
  if (view === null) { if (owner.children.delete(key)) draw(owner); return; }
  if (prior) { update(prior, view); return; }
  owner.children.set(key, slot(owner, null, view));
  draw(owner);
}

// Views with their own hosts, as the editor buttons in the head, while the
// session root owns their React children. Removing one must release its
// portal before removing the host.
export function setSessionPortal(host: HTMLElement, view: ReactNode) {
  const owner = current;
  if (!owner) return;
  const prior = owner.portals.get(host);
  if (prior) { update(prior, view); return; }
  owner.portals.set(host, slot(owner, host, view));
  draw(owner);
}

export function removeSessionPortal(host: HTMLElement) {
  const owner = current;
  if (!owner?.portals.delete(host)) return;
  draw(owner);
}

export const editHost = createRef<HTMLSpanElement>();

/// Where the head shows the Open in editor button, while it shows.
export const sessionEditHost = () => editHost.current?.isConnected ? editHost.current : null;

/// The log body the frame shows, a session's or a draft's, while it is in the page.
export function sessionBody(): ChatBody | null {
  const body = current?.frame.body;
  return body?.isConnected ? body : null;
}


/// The session's message field while the session page shows it.
export const agentMessage = createRef<HTMLTextAreaElement>();
/// The new-agent field while an empty agent view (a draft) shows it.
export const draftMessage = createRef<HTMLTextAreaElement>();

/// The field that takes the keys back from the pane: the session's or the draft's, while shown.
export const shownMessage = () => [agentMessage.current, draftMessage.current].find(field => field?.isConnected) ?? null;

export function showSessionFrame(frame: Frame, inPlace: boolean, release: () => void) {
  if (!inPlace || !current?.page.host.isConnected) {
    const page = openBarPage(() => { const old = current; current = null; old?.release(); });
    const store = createStore<FrameState>(() => ({ frame, revision: 1, slots: [] }));
    const owner: Owner = { page, store, revision: 1, frame, portals: new Map(), children: new Map(), next: 0, release };
    current = owner;
    flushSync(() => page.show({ view: "session", props: { store } }));
    return;
  }

  // The page it replaces lets go of what it held, as a draft its field.
  const owner = current, previous = owner.release;
  owner.release = release;
  previous();
  owner.frame = frame;
  owner.revision++;
  draw(owner);
}

const sameHints = (a: AgentHintsModel, b: AgentHintsModel) => a.project === b.project && a.value.window === b.value.window
  && a.value.items.length === b.value.items.length && a.value.items.every((item, i) => item.join("\n") === b.value.items[i].join("\n"));

export function updateSessionHints(hints: AgentHintsModel) {
  if (!current || sameHints(current.frame.hints, hints)) return;
  current.frame = { ...current.frame, hints };
  draw(current);
}
