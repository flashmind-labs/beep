import { FolderPicker } from "../addproject/folderbrowser.tsx";
import type { CopyLine, FolderOffer } from "../../parts/project/addmachine-state.ts";
import type { MachineFolder } from "../../stores/project/projects-state.ts";
import { Button, Spinner } from "../../ui/kit.tsx";

interface Props {
  title: string; verb: string; project: string; machine: string; start: string | null; adding: boolean; error: string | null; copying: CopyLine | null;
  offers(listing: MachineFolder, opened: string): FolderOffer[]; noteFor(listing: MachineFolder, offer: FolderOffer | null): Promise<string | null>; take(offer: FolderOffer): void; close(): void;
}
export function AddMachineView(p: Props) {
  if (p.copying) return <div className="fp-wait"><h5>{p.title}</h5><div className="am-wait"><Spinner />{p.copying.text}</div>{p.copying.pct !== null && <div className="fs-bar"><i style={{ width: `${p.copying.pct}%` }} /></div>}{p.copying.file && <div className="am-sub">{p.copying.file}</div>}{p.copying.note && <div className="am-sub">{p.copying.note}</div>}<Button look="quiet" onClick={p.close}>Close</Button></div>;

  return <FolderPicker machine={p.machine} start={p.start} heading={p.title} verb={p.verb} prefer={p.project} offers={p.offers} noteFor={p.noteFor} take={p.take} close={p.close} taking={p.adding} error={p.error} />;
}
