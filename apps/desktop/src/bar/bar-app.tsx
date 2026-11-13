// The bar's one React tree, rooted in a box in `#bar`: the page on screen,
// picked from the page store with a switch.
import type { ComponentProps } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { createStore, useStore } from "zustand";
import { bar } from "./core.ts";
import { WaitingList } from "./waiting-view.tsx";
import { SettingsBarShell, SettingsFailure } from "../settings/settings-shell-view.tsx";
import { MachineList, MachineLoading } from "../machines/list-view.tsx";
import { MachineDetail } from "../machines/detail-view.tsx";
import { AgentsList } from "../agents/agents-list-view.tsx";
import { AgentNewShell } from "../agents/agentnew-shell.tsx";
import { SessionFrame } from "../agents/session-frame.tsx";

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
const pages = createStore<{ id: number; page: BarPage }>(() => ({ id: 0, page: { view: "none" } }));
let app: { host: HTMLElement; root: Root } | null = null;
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

/// The page box, mounted on first use. When something replaced the bar's
/// contents, the next draw mounts a new box with the page on screen.
function mount(): HTMLElement {
  // `#bar` itself may move between hosts; only a box taken out of it is gone.
  if (app?.host.parentElement === bar) return app.host;

  app?.root.unmount();
  const host = document.createElement("div");
  host.style.display = "contents";
  host.dataset.barPage = "";
  bar.replaceChildren(host);
  const root = createRoot(host);
  flushSync(() => root.render(<BarApp />));
  app = { host, root };
  return host;
}

function BarApp() {
  const { id, page } = useStore(pages);
  return <Page key={id} page={page} />;
}

function Page({ page }: { page: BarPage }) {
  switch (page.view) {
    case "none": return null;
    case "agents": return <AgentsList {...page.props} />;
    case "agent-new": return <AgentNewShell {...page.props} />;
    case "session": return <SessionFrame {...page.props} />;
    case "waiting": return <WaitingList {...page.props} />;
    case "settings": return <SettingsBarShell {...page.props} />;
    case "settings-failure": return <SettingsFailure error={page.error} />;
    case "machines": return <MachineList {...page.props} />;
    case "machine": return <MachineDetail {...page.props} />;
    case "machine-loading": return <MachineLoading />;
  }
}
