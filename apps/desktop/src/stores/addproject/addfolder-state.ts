// What the "Add a folder" sheet decides, as pure functions so they can be
// tested without Tauri: the machine it starts on and what a folder offers.
import { repoFromRemote } from "./importproject-state.ts";
import type { MachineFolder } from "../project/projects-state.ts";
import type { FolderOffer } from "../../parts/project/addmachine-state.ts";

/// The machine the sheet opens on, of those online: this computer, else the first.
export function startOn(machines: readonly { id: string }[], macId: string | null): string | null {
  return machines.find(m => m.id === macId)?.id ?? machines[0]?.id ?? null;
}

/// A checkout with an origin remote is added in place, named `org/repo` from it; no other
/// folder offers anything.
export function folderOffers(listing: MachineFolder): FolderOffer[] {
  if (!listing.origin) return [];

  const name = repoFromRemote(listing.origin).replace(/\.git\/?$/, "").split(/[/:]/).filter(Boolean).slice(-2).join("/");
  return [{ label: `Add ${name}`, path: listing.path }];
}

/// Why a checkout offers nothing; null for a plain folder, which holds the ones to open.
export function noOffer(listing: MachineFolder) {
  return listing.origin === "" ? "This checkout has no origin remote. Add one to add it here." : null;
}
