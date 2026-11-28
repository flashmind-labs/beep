import { flushSync } from "react-dom";
import { AddFolderView } from "./addfolder-view.tsx";
import { syncthingImportChoice } from "../project/syncthing.ts";
import { SyncthingChoice } from "../project/syncthing-choice.tsx";
// The "Add a folder" sheet: a machine (this computer first) and a palette of
// its folders, opened in the folder it keeps projects in. A checkout with an
// origin remote offers Add, which adds it in place with no clone.
import { invoke } from "../../ui/tauri.ts";
import { openSheet } from "../../ui/parts.tsx";
import { keepLastImport, projectsIn } from "../../stores/addproject/importproject-state.ts";
import { cleanError, repoFromRemote } from "../../stores/addproject/importproject-state.ts";
import { noOffer, startOn } from "../../stores/addproject/addfolder-state.ts";
import type { FolderOffer } from "../../parts/project/addmachine-state.ts";
import type { MachineFolder, ProjectRow } from "../../stores/project/projects-state.ts";

/// What the window hands the sheet.
interface AddFolderOptions {
  /// The machines Home lists.
  machines?: readonly { id: string; name: string; online: boolean; can_host?: boolean }[];
  /// This computer's machine id, when it is one.
  macId?: string | null;
  /// Runs once the project is in.
  onDone?: ((project: ProjectRow) => void) | null;
  /// Back to the Add a project sheet.
  onBack?: (() => void) | null;
}

/// Opens the sheet; each opening has its own state.
export function openAddFolder(options: AddFolderOptions = {}) {
  const machines = (options.machines ?? []).filter(m => m.online && m.can_host !== false);
  const syncthing = syncthingImportChoice();
  /// Where each machine keeps its projects, once read.
  const starts = new Map<string, string>();
  let machine: string | null = null, adding = false, error: string | null = null, open = true;
  const { close, render } = openSheet("Add a folder", "cc-pal fp", {
    canClose: () => !adding,
    onClose: () => { open = false; syncthing.dispose(); },
  });

  const draw = () => {
    if (!open) return;
    flushSync(() => render(<AddFolderView
      machines={machines} machine={machine} start={machine ? starts.get(machine) ?? null : null} adding={adding} error={error}
      pick={pick} noteFor={noteFor} add={listing => void add(listing)} close={() => { if (adding) return; close(); options.onBack?.(); }}
    />));
  };

  const pick = (id: string) => {
    if (adding || id === machine) return;
    machine = id;
    error = null;
    draw();
    if (!starts.has(id)) void projectsIn(invoke, id).then(root => { starts.set(id, root); draw(); });
  };

  /// The Syncthing choice, which shows only for a folder Syncthing holds; or why a checkout cannot be added.
  const noteFor = async (listing: MachineFolder, offer: FolderOffer | null) => {
    if (!offer) return noOffer(listing);

    void syncthing.follow(machine, offer.path);
    return <SyncthingChoice choice={syncthing} />;
  };

  const add = async (listing: MachineFolder) => {
    if (adding || !machine) return;
    adding = true;
    error = null;
    draw();

    try {
      const project = await invoke<ProjectRow>("import_project", {
        repo: repoFromRemote(listing.origin), machineId: machine, folder: listing.path, worktrees: [], parent: null, syncthingFiles: syncthing.value(),
      });
      keepLastImport(project);
      adding = false;
      close();
      options.onDone?.(project);
    } catch (e) {
      adding = false;
      error = cleanError(String(e));
      draw();
    }
  };

  const first = startOn(machines, options.macId ?? null);
  if (first) pick(first);
  else draw();
}
