import { useStore } from "zustand";
import type { SyncthingImportChoice } from "./syncthing.ts";
import { Checkbox } from "../../ui/kit.tsx";

/// The Syncthing choice of an import; nothing where Syncthing holds none of it.
export function SyncthingChoice({ choice }: { choice: SyncthingImportChoice }) {
  const state = useStore(choice.state);
  if (!state.available) return null;

  return <div className="st-choice"><label className="am-line"><Checkbox checked={state.checked} mixed={state.indeterminate} change={checked => choice.change(checked)} />Keep Syncthing file sync</label><div className="am-sub">{state.label ? `In Syncthing's "${state.label}" folder. Turn this off to sync it with Beep instead.` : "Turn this off to sync what you import with Beep instead."}</div></div>;
}
