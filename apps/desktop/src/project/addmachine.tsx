import { createElement } from "react";
import { flushSync } from "react-dom";
import { AddMachineView } from "./addmachine-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
// The "Add <project> to <machine>" sheet of a project's "Where it lives": the
// folder there, then how far Beep's sync has brought the files. With `from`
// it is the "Change folder" sheet: the machine points at another folder,
// empty or missing (the others fill it) or a checkout of the same repository,
// and the folder it had stays on disk as it is.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.tsx";
import { reason } from "../ui/format.ts";
import { projectsIn } from "../addproject/importproject.tsx";
import { projectUnder } from "../addproject/importproject-state.ts";
import { browseStart, copyLine, destination, shortName, type CopyLine } from "./addmachine-state.ts";
import type { SyncDetail } from "./filesync.ts";
import { plainPath, type MachineFolder } from "./projects-state.ts";

/// What the page hands the sheet.
interface AddToMachine {
  project: string;
  machine: { id: string; name: string };
  /// This Mac's machine id: there Choose… opens the macOS folder picker.
  macId: string | null;
  /// The folder was made and the machine joined the sync.
  added: () => void;
  /// The folder the project is in there now, to change it.
  from?: string;
}

/// Opens the sheet; each opening has its own state, and its reads stop when it closes.
export function openAddToMachine(o: AddToMachine) {
  const { project, machine } = o;
  const name = shortName(project);
  const change = o.from != null;
  const title = change ? `Change the folder of ${name} on ${machine.name}` : `Add ${name} to ${machine.name}`;
  let folder = "", browsing: MachineFolder | null = null, reading = false, error: string | null = null;
  let adding = false, copying: CopyLine | null = null, open = true;
  let unfollow: (() => void) | undefined;
  const { close, render } = openSheet(title, "am-sheet ip-sheet ip-add", {
    onClose: () => { open = false; unfollow?.(); },
  });

  const draw = () => {
    if (!open) return;
    flushSync(() => render(createElement(AddMachineView, {
      title, project, machine: machine.name, from: o.from == null ? null : plainPath(o.from), folder, browsing, reading, adding, error, copying,
      edit: (value: string) => { folder = value; error = null; draw(); }, choose: () => void choose(), add: () => void add(), close,
      browse: (path: string | null) => void read(path, next => { browsing = next; }),
      cancelBrowse: () => { browsing = null; draw(); },
      useFolder: (listing: MachineFolder) => { folder = destination(listing, project, change); browsing = null; draw(); },
    })));
  };

  /// Reads a folder on the machine (`path` null for where it starts).
  const read = async (path: string | null, then: (listing: MachineFolder) => void) => {
    if (reading) return;
    reading = true;
    draw();

    try {
      const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine.id, path });
      if (open) { then(listing); error = null; }
    } catch (e) {
      if (open) error = reason(e);
    }

    reading = false;
    draw();
  };

  const choose = async () => {
    if (o.macId !== machine.id) {
      read(browseStart(folder), listing => { browsing = listing; });
      return;
    }

    try {
      const path = await invoke<string | null>("pick_folder", { start: browseStart(folder) });
      if (path && open) read(path, listing => { folder = destination(listing, project, change); });
    } catch (e) {
      error = reason(e);
      draw();
    }
  };

  const add = async () => {
    if (adding || !folder.trim()) return;
    adding = true;
    error = null;
    draw();

    try {
      await invoke<string>(change ? "project_change_folder" : "project_add_to_machine", { project, machineId: machine.id, folder });
      o.added();
      if (open) { copying = copyLine(null, machine.id, machine.name); void poll(); }
    } catch (e) {
      error = reason(e);
    }

    adding = false;
    draw();
  };

  /// Reads the sync until the machine has it all; the page's row goes on from there.
  const poll = async () => {
    try {
      const detail = await invoke<SyncDetail | null>("project_sync_detail", { project });
      copying = copyLine(detail, machine.id, machine.name);
    } catch (e) {
      console.warn("project sync:", e);
    }

    if (!open) return;
    if (copying?.done) { close(); return; }
    draw();
    unfollow ??= subscribeChanges(topic("sync"), { deliver: "latest", receive: () => poll() }, { machine: machine.id, when: () => open });
  };

  draw();

  // Changing starts where the folder is now.
  if (change) {
    read(browseStart(o.from!), listing => { browsing = listing; });
    return;
  }

  // The field starts at the machine's default, unless the user got there first.
  void projectsIn(machine.id).then(root => {
    if (open && !folder) { folder = projectUnder(root, project); draw(); }
  });
}
