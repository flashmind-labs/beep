import { useLayoutEffect, useRef } from "react";
import { useStore } from "zustand";
import type { SyncthingImportChoice } from "./syncthing.ts";

export function SyncthingChoice({ choice, className = "" }: { choice: SyncthingImportChoice; className?: string }) {
  const state = useStore(choice.state);
  const checkbox = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => { if (checkbox.current) checkbox.current.indeterminate = state.indeterminate; }, [state.indeterminate]);
  return <div><label className="am-line"><input ref={checkbox} className={className} type="checkbox" checked={state.checked} disabled={state.disabled} onChange={event => choice.change(event.currentTarget.checked)} />Keep Syncthing file sync</label><div className="am-sub">If Syncthing manages this folder, turn this off to use Beep on all machines. Git projects sync commits; uncommitted edits need a handoff. Other projects keep syncing.</div></div>;
}
