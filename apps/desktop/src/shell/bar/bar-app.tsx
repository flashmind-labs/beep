// The bar's one React tree, rooted in the page box: the page on screen,
// picked from the page store with a switch.
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { useStore } from "zustand";
import { pageBox, pages, type BarPage } from "../../stores/bar/bar-page.ts";
import { WaitingList } from "../../parts/bar/waiting-view.tsx";
import { SettingsBarShell, SettingsFailure } from "../../screens/settings/settings-shell-view.tsx";
import { MachineList, MachineLoading } from "../../screens/machines/list-view.tsx";
import { MachineDetail } from "../../screens/machines/detail-view.tsx";
import { AgentsList } from "../../parts/session/agents-list-view.tsx";
import { AgentNewShell } from "../../parts/session/agentnew-shell.tsx";
import { SessionFrame } from "../../parts/session/session-frame-view.tsx";

let root: Root | null = null;

// A new page box gets a new tree, drawn before the box is handed back.
pageBox.subscribe(({ host }) => {
  root?.unmount();
  root = null;
  if (!host) return;
  const next = createRoot(host);
  root = next;
  flushSync(() => next.render(<BarApp />));
});

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
