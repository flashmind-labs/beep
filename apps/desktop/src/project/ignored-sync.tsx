import { useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../ui/parts.ts";
import { invoke } from "../ui/tauri.ts";
import { reason } from "../ui/format.ts";
import { ignoredSelection, type IgnoredSync } from "./ignored-sync-state.ts";

export function chooseIgnoredFolders(project: string, initial: IgnoredSync, onChanged: () => void) {
  let busy = false;
  const { close, render } = openSheet("Sync gitignored", "am-sheet", { canClose: () => !busy });
  const save = async (enabled: boolean, paths: string, setError: (message: string) => void, setBusy: (value: boolean) => void) => {
    if (busy) return;
    try {
      const settings = ignoredSelection(enabled, paths);
      busy = true;
      setBusy(true);
      await invoke("project_ignored_sync", { project, settings });
      busy = false;
      close();
      onChanged();
    } catch (error) {
      busy = false;
      setError(reason(error));
      setBusy(false);
    }
  };
  flushSync(() => render(<IgnoredSyncView initial={initial} save={save} cancel={() => { if (!busy) close(); }} />));
}

function IgnoredSyncView({ initial, save, cancel }: { initial: IgnoredSync; save(enabled: boolean, paths: string, setError: (message: string) => void, setBusy: (value: boolean) => void): void; cancel(): void }) {
  const [enabled, setEnabled] = useState(initial.enabled), [paths, setPaths] = useState(initial.folders.join("\n"));
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const textarea = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { textarea.current?.focus(); }, []);
  return <><div className="am-head"><b>Sync gitignored</b><kbd>esc</kbd></div>
    <label className="am-line"><input type="checkbox" checked={enabled} disabled={busy} onChange={event => setEnabled(event.currentTarget.checked)} />Sync selected ignored folders on all machines</label>
    <div className="am-label">Folders relative to the project, one per line</div><textarea ref={textarea} className="ip-path" rows={4} value={paths} placeholder={"assets/models\ndata/captures"} aria-label="Folders relative to the project, one per line" disabled={busy} onChange={event => setPaths(event.currentTarget.value)} />
    <div className="am-sub">Each folder must be wholly ignored by Git and contain no tracked or staged files. These folders use file sync, including conflict copies. Other uncommitted edits stay local. Build output, linked repositories and paths excluded in .beepignore stay local.</div>
    <div className="am-sub">Beep waits while Syncthing manages these folders. Turning this off leaves local files in place.</div>
    <div className="am-msg bad" role="status" hidden={!error}>{error}</div><div className="am-line ip-acts"><button type="button" className="dbtn quiet" onClick={cancel}>Cancel</button><button type="button" className="dbtn pri" disabled={busy} onClick={() => save(enabled, paths, setError, setBusy)}>{busy ? "Saving…" : "Save"}</button></div></>;
}
