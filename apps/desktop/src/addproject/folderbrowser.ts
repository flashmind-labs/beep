// The folder browser two sheets share, for a machine other than this Mac: the
// folder, its folders, and what picking it does.
import { button, el } from "../ui/dom.ts";
import type { MachineFolder } from "../project/projects-state.ts";

/// What the sheet around the browser gives it.
export interface FolderBrowser {
  /// A folder is being read, so the folder buttons wait.
  reading: boolean;
  /// What picking this folder does.
  note: string | null;
  /// Moves to a folder.
  go: (path: string | null) => void;
  cancel: () => void;
  use: () => void;
}

export function folderBrowser(listing: MachineFolder, b: FolderBrowser) {
  const up = button("Up", "dbtn quiet ip-up", () => b.go(listing.parent));
  up.disabled = !listing.parent || b.reading;
  const where = el("span", "", listing.path);
  where.title = listing.path;
  const into = (name: string) => `${listing.path.replace(/\/+$/, "")}/${name}`;
  const dirs = el("div", "dirs", ...listing.folders.map(name => {
    const row = button(name, "", () => b.go(into(name)));
    row.disabled = b.reading;
    return row;
  }));
  if (!listing.folders.length) dirs.append(el("span", "none", listing.exists ? "No folders in here" : "Not there yet"));
  if (listing.more) dirs.append(el("span", "none", `and ${listing.more} more`));
  const foot = el("div", "foot", el("span", "note", b.note),
    button("Cancel", "dbtn quiet", b.cancel), button("Use this folder", "dbtn pri", b.use));
  return el("div", "ip-browse", el("div", "at", up, where), dirs, foot);
}
