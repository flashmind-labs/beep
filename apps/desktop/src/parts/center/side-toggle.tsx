import { useStore } from "zustand";
import { shell, sideCollapsed, toggleSide } from "./window-shell.tsx";

/// The button that folds the sidebar, or beside a project's session swaps it with the project's
/// sessions. The sidebar and the sessions each show it in the same place.
export function SideToggle() {
  const side = useStore(shell, s => s.side), collapsed = sideCollapsed(side);
  const label = side.rail ? (side.railHidden ? (side.rail.kind === "chat" ? "Show the chat's threads" : "Show the project's sessions") : "Show the sidebar") : collapsed ? "Expand sidebar" : "Collapse sidebar";
  return <button type="button" className="cc-side-toggle" title={label} aria-label={label} aria-expanded={!collapsed} onClick={toggleSide}><span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></svg></span></button>;
}
