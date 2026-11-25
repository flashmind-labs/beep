// What a project's name does wherever it shows: pin it, open it in a window, its menu, remove it.
import { type MouseEvent as ReactMouseEvent } from "react";
import { invoke, openUrl } from "../ui/tauri.ts";
import { onPhone } from "../ui/platform.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.tsx";
import { MACHINES } from "../machines/machine-store.ts";
import { whereRows, openOnLabel, removeProjectSheet, REMOVE_PROJECT } from "./projects-state.ts";
import { openInEditor } from "./editors.ts";
import { NEW_WINDOW, opensNewWindow, type Alone, type ProjectWindowProps } from "./projectwindow.ts";
import { prefetchOnHover } from "./loads.tsx";
import { openProjects, loadFound } from "./addflow.ts";
import { projectRows } from "./projectrows.ts";
import { state, whereOf, homeDataChanged, listsChanged, projectLoaded } from "../center/state.ts";
import { currentLocation } from "../center/nav.ts";
import { forgetProject } from "../center/homeedit.ts";

/// Pins project `name` to the top of the sidebar, or unpins it. Not the
/// file sync pin, which keeps a project on the machines it is on.
export async function setProjectOnTop(name: string, pinned: boolean) {
  const before = state.top;
  state.top = pinned ? [...before.filter(n => n !== name), name] : before.filter(n => n !== name);
  redrawProjectPins();

  try {
    await invoke("pin_project_to_top", { project: name, pinned });
  } catch (e) {
    console.warn("pinning a project:", e);
    state.top = before;
    redrawProjectPins();
  }
}

function redrawProjectPins() {
  listsChanged.notify();
  projectLoaded.notify();
}

/// Opens project `name` in a window of its own, or brings up the one it
/// already has.
export function openProjectWindow(name: string) {
  openWindow({ kind: "project", project: name });
}

export function openWindow(alone: Alone) {
  invoke("center_open_window", { alone }).catch(e => console.warn("opening a window:", e));
}

/// What lets project `name` open in a window of its own: ⌘-click or middle-click on the
/// element given `clicks`, which also reads the project ahead on hover, or "Open in New Window"
/// from a right-click on the one given `menu`. The capture runs before the click opens it here.
export function projectWindowProps(name: string): ProjectWindowProps {
  const open = (e: ReactMouseEvent) => {
    if (onPhone() || !opensNewWindow(e)) return;
    e.preventDefault();
    e.stopPropagation();
    openProjectWindow(name);
  };
  return {
    clicks: { onClickCapture: open, onAuxClick: open, ...prefetchOnHover(name) },
    menu: { onContextMenu: (e: ReactMouseEvent) => { e.stopPropagation(); openProjectMenu(e.nativeEvent, name); } },
  };
}

/// The right-click menu of project `name`: a window of its own, and
/// removing it once it is imported (one from running agents only has
/// nothing to remove).
function openProjectMenu(e: MouseEvent, name: string) {
  const p = projectRows()?.find(x => x.name === name);
  const on = state.top.includes(name);
  const items: MenuItem[] = [
    ...(onPhone() ? [] : [{ label: NEW_WINDOW, icon: "openWindow" as const, on: () => openProjectWindow(name) }]),
    { label: on ? "Unpin" : "Pin to top", icon: "pin", on: () => setProjectOnTop(name, !on) },
  ];
  const folder = p && whereRows(p, whereOf(name) || null, MACHINES.mac?.id || null).find(r => r.mac && r.folder)?.folder;
  if (folder && !onPhone()) items.push({ label: "Open in editor", icon: "paneText", on: () => void openInEditor(folder) });
  const label = p?.url ? openOnLabel(p.url) : null;
  if (p?.url && label) items.push({ label, icon: "openWindow", on: () => openUrl(p.url!) });
  if (p?.imported) items.push({ label: "Rename", icon: "edit", on: () => void import("../project/rename.tsx").then(m => m.renameProject(p)) });
  if (p?.imported) items.push({ kind: "separator" }, { label: REMOVE_PROJECT, icon: "remove", warn: true, on: () => removeProject(name) });
  openMenu(e, name, items);
}

/// Asks, then removes project `name` from Beep. Its files stay on every
/// machine.
export function removeProject(name: string) {
  confirmSheet({
    ...removeProjectSheet(name),
    run: async () => {
      await invoke("remove_project", { project: name });
      forgetProject(name);

      if (currentLocation()?.view === "projects" && state.picked === name) openProjects();
      else { homeDataChanged.notify(); projectLoaded.notify(); }

      loadFound();
    },
  });
}
