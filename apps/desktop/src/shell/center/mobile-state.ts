// The phone layout's tabs and Back, apart from the DOM so they can be tested.
import type { ViewName } from "../../stores/center/state.ts";

export type MobileTab = "home" | "projects" | "agents" | "terminals" | "machines";

export const TABS: readonly { id: MobileTab; label: string }[] = [
  { id: "home", label: "Home" },
  { id: "projects", label: "Projects" },
  { id: "agents", label: "Agents" },
  { id: "terminals", label: "Terminals" },
  { id: "machines", label: "Machines" },
];

/// Where the window is.
export interface MobileAt {
  page: ViewName | null;
  /// The pane's view when the page is the pane.
  paneView: string | null;
  homeTab: string;
  /// The project the Projects page shows; null for the list.
  picked: string | null;
  /// A tile of the project is open.
  tile: boolean;
  /// A terminal or the New terminal form is open on the Terminals page.
  terminal: boolean;
}

/// The tab lit for `at`; null on a page no tab owns.
export function activeTab(at: MobileAt): MobileTab | null {
  if (at.page === "home") return at.homeTab === "machines" ? "machines" : "home";
  if (at.page === "projects") return "projects";
  if (at.page === "terminals") return "terminals";
  if (at.page === "pane" && (at.paneView === "agents" || at.paneView === "agent")) return "agents";
  return null;
}

export type BackTo = "overview" | "tile" | "projects" | "terminals" | "history";

/// What Back does from `at`; null on the first page of a tab, where there is no Back.
export function backTo(at: MobileAt): BackTo | null {
  if (!at.page) return null;

  switch (at.page) {
    case "home": return at.homeTab === "git" ? "overview" : null;
    case "projects": return at.tile ? "tile" : at.picked ? "projects" : null;
    case "terminals": return at.terminal ? "terminals" : null;
    case "pane": return at.paneView === "agents" ? null : "history";
    default: return "history";
  }
}

/// The page fills the screen with no tab bar: a session or an open terminal.
export function fullScreen(at: MobileAt): boolean {
  return (at.page === "pane" && at.paneView === "agent") || (at.page === "terminals" && at.terminal);
}
