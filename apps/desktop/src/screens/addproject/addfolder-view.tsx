import type { ReactNode } from "react";
import { FolderPicker } from "./folderbrowser.tsx";
import { folderOffers } from "../../stores/addproject/addfolder-state.ts";
import type { FolderOffer } from "../../parts/project/addmachine-state.ts";
import type { MachineFolder } from "../../stores/project/projects-state.ts";
import { ChipToggle } from "../../ui/kit.tsx";

interface Props {
  /// The machines online, the one picked, and the folder its picker opens in once read.
  machines: readonly { id: string; name: string }[]; machine: string | null; start: string | null; adding: boolean; error: string | null;
  pick(id: string): void; noteFor(listing: MachineFolder, offer: FolderOffer | null): Promise<ReactNode>; add(listing: MachineFolder): void; close(): void;
}
export function AddFolderView(p: Props) {
  if (!p.machine) return <div className="fp-wait"><h5>Add a folder</h5><div className="am-sub">No machine is online.</div></div>;

  return <>{p.machines.length > 1 && <div className="dchips fp-on">{p.machines.map(m => <ChipToggle key={m.id} label={m.name} selected={m.id === p.machine} onToggle={() => p.pick(m.id)} />)}</div>}
    {p.start !== null && <FolderPicker key={p.machine} machine={p.machine} start={p.start} heading="Add a folder" verb="Add" prefer="" offers={folderOffers} noteFor={p.noteFor} take={(_, listing) => p.add(listing)} close={p.close} taking={p.adding} error={p.error} />}</>;
}
