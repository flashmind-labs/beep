import { flushSync } from "react-dom";
import { AddMachineView } from "./addmachine-view.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
// The "Add <project> to <machine>" sheet of a project's "Where it lives": a
// picker of the folder there, then how far Beep's sync has brought the files.
// With `from` it is the "Change folder" sheet: the machine points at another
// folder and the one it had stays on disk as it is. The desk decides what a
// folder may be; `offers` says what the picker proposes.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.tsx";
import { reason } from "../ui/format.ts";
import { projectsIn } from "../addproject/importproject.tsx";
import { browseStart, copyLine, offers, shortName, type CopyLine, type FolderOffer } from "./addmachine-state.ts";
import type { SyncDetail } from "./filesync.ts";
import { plainPath } from "./projects-state.ts";
import type { SyncthingFiles } from "./syncthing.ts";

/// What the page hands the sheet.
interface AddToMachine {
  project: string;
  machine: { id: string; name: string };
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
  let start: string | null | undefined, error: string | null = null;
  let adding = false, copying: CopyLine | null = null, open = true;
  let unfollow: (() => void) | undefined;
  const { close, render } = openSheet(title, "cc-pal fp", {
    onClose: () => { open = false; unfollow?.(); },
  });

  const draw = () => {
    const at = start;
    if (!open || at === undefined) return;
    flushSync(() => render(<AddMachineView
      title={title} verb={change ? "Change" : "Add"} project={name} machine={machine.id} start={at} adding={adding} error={error} copying={copying}
      offers={(listing, opened) => offers(listing, project, change, change ? null : opened)} noteFor={noteFor} take={offer => void add(offer)} close={close}
    />));
  };

  /// Beep takes a folder Syncthing keeps out of its share; the picker says so before it does.
  const noteFor = async (offer: FolderOffer) => {
    const files = await invoke<SyncthingFiles>("project_syncthing_files", { machineId: machine.id, folder: offer.path });
    if (!files.available || !files.enabled) return null;
    const share = files.label ? `Syncthing's "${files.label}" folder` : "a Syncthing folder";
    return `This is in ${share}. Beep adds it to Syncthing's ignore list on ${machine.name} and syncs it itself.`;
  };

  const add = async (offer: FolderOffer) => {
    if (adding) return;
    adding = true;
    error = null;
    draw();

    try {
      await invoke<string>(change ? "project_change_folder" : "project_add_to_machine", { project, machineId: machine.id, folder: offer.path });
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

  // Changing starts where the folder is now, adding in the machine's projects folder.
  if (o.from != null) {
    start = browseStart(plainPath(o.from));
    draw();
    return;
  }

  void projectsIn(machine.id).then(root => { start = root; draw(); });
}
