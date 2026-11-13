import { useStore } from "zustand";
import type { SyncthingImportChoice } from "./syncthing.ts";
import { Checkbox } from "../ui/kit.tsx";

export function SyncthingChoice({ choice }: { choice: SyncthingImportChoice }) {
  const state = useStore(choice.state);
  return <div><label className="am-line"><Checkbox checked={state.checked} mixed={state.indeterminate} disabled={state.disabled} change={checked => choice.change(checked)} />Keep Syncthing file sync</label><div className="am-sub">If Syncthing manages this folder, turn this off to use Beep on all machines. Git projects sync commits; uncommitted edits need a handoff. Other projects keep syncing.</div></div>;
}
