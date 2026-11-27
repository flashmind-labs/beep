import { flushSync } from "react-dom";
import { AddMachineView } from "./addmachine-view.tsx";
import { subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedView } from "../ui/snapshot-state.ts";
// The "Add <project> to <machine>" sheet of a project's "Where it lives": a
// picker of the folder there, then how far Beep's sync has brought the files.
// With `from` it is the "Change folder" sheet: the machine points at another
// folder and the one it had stays on disk as it is. The desk decides what a
// folder may be; `offers` says what the picker proposes.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.tsx";
import { reason } from "../ui/format.ts";
import { projectsIn } from "../addproject/importproject-state.ts";
import { browseStart, copyLine, offers, shortName, type CopyLine, type FolderOffer } from "./addmachine-state.ts";
import type { SyncDetail } from "./filesync.ts";
import { plainPath } from "./filesync.ts";
import type { SyncthingFiles } from "./syncthing.ts";

/// What the page hands the sheet.
interface AddToMachine {
  project: string;
  machine: { id: string; name: string };
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
  const noteFor = async (_: unknown, offer: FolderOffer | null) => {
    if (!offer) return null;

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
      if (open) { copying = copyLine(null, machine.id, machine.name); void poll(); }
    } catch (e) {
      error = reason(e);
    }

    adding = false;
    draw();
  };

  /// Reads the sync once, then follows it until the machine has it all; the page's row goes on from there.
  /// The views are published under this desktop, so no machine filter.
  const poll = async () => {
    try {
      take(await invoke<SyncDetail | null>("project_sync_detail", { project }));
    } catch (e) {
      console.warn("project sync:", e);
      if (open) draw();
    }

    if (!open) return;
    unfollow ??= subscribeEach(topic("sync"), event => {
      const pushed = pushedView<SyncDetail | null>(event.data, "project_sync_detail", { project });
      if (pushed && "value" in pushed) take(pushed.value);
    }, { when: () => open });
  };

  const take = (detail: SyncDetail | null) => {
    copying = copyLine(detail, machine.id, machine.name);
    if (!open) return;
    if (copying.done) close(); else draw();
  };

  // Changing starts where the folder is now, adding in the machine's projects folder.
  if (o.from != null) {
    start = browseStart(plainPath(o.from));
    draw();
    return;
  }

  void projectsIn(machine.id).then(root => { start = root; draw(); });
}
