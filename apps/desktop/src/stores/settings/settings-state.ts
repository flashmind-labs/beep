import { S } from "../bar/core.ts";
import type { MacControl } from "../../parts/machines/machine-state.ts";
import type { MacSettings } from "../../screens/settings/settings.tsx";
import type { AgentsTab } from "../../screens/settings/settings-shell-view.tsx";
import type { SettingsPath } from "./settings-nav.ts";

/// Settings: drawn at once from what this Mac knows (its own settings and the
/// last machines seen), then the machines and credentials fill in as they answer.
export const SET: {
  root: HTMLElement | null;
  token: number;
  path: SettingsPath | null;
  agentsTab: AgentsTab;
  local: { set: MacSettings; mac: MacControl | null } | null;
  unembed: (() => void) | null;
} = { root: null, token: 0, path: null, agentsTab: "accounts", local: null, unembed: null };

/// The sheet is open, or the bar still shows Settings.
export const setShown = () => SET.root ? SET.root.isConnected : S.view === "settings";

const changed = new Set<() => void>();

/// `redraw` runs when something outside Settings changed what it shows.
export function onSettingsChanged(redraw: () => void) {
  changed.add(redraw);
}

/// Draws Settings again where it is shown, the sheet or the bar.
export function settingsChanged() {
  for (const redraw of changed) redraw();
}
