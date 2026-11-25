// The bar's page store: what is on screen, and the box it draws in.
import type { ComponentProps } from "react";
import { flushSync } from "react-dom";
import { createStore } from "zustand";
import { bar } from "./core.ts";
import type { WaitingList } from "./waiting-view.tsx";
import type { SettingsBarShell } from "../settings/settings-shell-view.tsx";
import type { MachineList } from "../machines/list-view.tsx";
import type { MachineDetail } from "../machines/detail-view.tsx";
import type { AgentsList } from "../agents/agents-list-view.tsx";
import type { AgentNewShell } from "../agents/agentnew-shell.tsx";
import type { SessionFrame } from "../agents/session-frame-view.tsx";

/// What the bar shows, with what it draws from.
export type BarPage =
  | { view: "none" }
  | { view: "agents"; props: ComponentProps<typeof AgentsList> }
  | { view: "agent-new"; props: ComponentProps<typeof AgentNewShell> }
  | { view: "session"; props: ComponentProps<typeof SessionFrame> }
  | { view: "waiting"; props: ComponentProps<typeof WaitingList> }
  | { view: "settings"; props: ComponentProps<typeof SettingsBarShell> }
  | { view: "settings-failure"; error: string }
  | { view: "machines"; props: ComponentProps<typeof MachineList> }
  | { view: "machine"; props: ComponentProps<typeof MachineDetail> }
  | { view: "machine-loading" };

/// A page opened in the bar. It draws while no other page has opened since.
export interface BarPageSlot {
  /// The bar's page box, the same element for every page.
  readonly host: HTMLElement;
  isOpen(): boolean;
  show(page: BarPage): void;
}

/// The page on screen; `id` counts openings, so a page opened again starts fresh.
export const pages = createStore<{ id: number; page: BarPage }>(() => ({ id: 0, page: { view: "none" } }));
/// The page box. A new one is announced here, and the bar's React tree mounts in it.
export const pageBox = createStore<{ host: HTMLElement | null }>(() => ({ host: null }));
let opened: { id: number; release: () => void } | null = null;

/// Opens a page in the bar. The page before is released and unmounted first;
/// `release` runs when the next page opens. Callers draw inside `flushSync`
/// when they read the page's elements right after.
export function openBarPage(release: () => void): BarPageSlot {
  const box = mount();
  const before = opened;
  const id = pages.getState().id + 1;
  opened = { id, release };
  before?.release();
  flushSync(() => pages.setState({ id, page: { view: "none" } }));

  const isOpen = () => opened?.id === id;
  const show = (page: BarPage) => {
    if (!isOpen()) return;
    mount();
    pages.setState({ page });
  };
  return { host: box, isOpen, show };
}

/// The page box, made on first use. When something replaced the bar's
/// contents, the next draw makes a new box with the page on screen.
function mount(): HTMLElement {
  // `#bar` itself may move between hosts; only a box taken out of it is gone.
  const shown = pageBox.getState().host;
  if (shown?.parentElement === bar) return shown;

  const host = document.createElement("div");
  host.style.display = "contents";
  host.dataset.barPage = "";
  bar.replaceChildren(host);
  pageBox.setState({ host });
  return host;
}
