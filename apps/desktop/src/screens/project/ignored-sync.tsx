import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../../ui/parts.tsx";
import { invoke } from "../../ui/tauri.ts";
import { reason } from "../../ui/format.ts";
import { gitignoreSuggestions, ignoredSelection, listedFolders, type GitignoreSuggestions, type IgnoredSync } from "../../stores/project/ignored-sync-state.ts";
import { Button, Checkbox, ChipToggle, NO_AUTO, SheetHead } from "../../ui/kit.tsx";
import { readFile } from "../../stores/agentpane/read-file.ts";
import type { ProjectPlace } from "../../stores/project/projects-state.ts";

/// `places` are where the project is: the first `.gitignore` read there
/// offers its folders.
export function chooseIgnoredFolders(project: string, places: readonly ProjectPlace[], initial: IgnoredSync, onChanged: () => void) {
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
  flushSync(() => render(<IgnoredSyncView initial={initial} places={places} save={save} cancel={() => { if (!busy) close(); }} />));
}

/// Folder chips shown before "more".
const SHOWN = 8;
const NONE: GitignoreSuggestions = { folders: [], files: [] };

function IgnoredSyncView({ initial, places, save, cancel }: { initial: IgnoredSync; places: readonly ProjectPlace[]; save(enabled: boolean, paths: string, setError: (message: string) => void, setBusy: (value: boolean) => void): void; cancel(): void }) {
  const [enabled, setEnabled] = useState(initial.enabled), [paths, setPaths] = useState(initial.folders.join("\n"));
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [suggestions, setSuggestions] = useState<GitignoreSuggestions>(NONE), [all, setAll] = useState(false);
  const textarea = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { textarea.current?.focus(); }, []);
  useEffect(() => {
    let open = true;
    Promise.any(places.map(place => readFile(place.id, `${place.folder}/.gitignore`, null, () => {})))
      .then(file => gitignoreSuggestions(new TextDecoder().decode(file.bytes)), () => NONE)
      .then(found => { if (open) setSuggestions(found); });
    return () => { open = false; };
  }, [places]);

  const listed = listedFolders(paths);
  const toggle = (folder: string) => setPaths(listed.includes(folder) ? listed.filter(p => p !== folder).join("\n") : [...listed, folder].join("\n"));

  return <><SheetHead title="Sync gitignored" />
    <label className="am-line"><Checkbox checked={enabled} disabled={busy} change={setEnabled} />Sync selected ignored folders on all machines</label>
    <div className="am-label">Folders relative to the project, one per line</div><textarea ref={textarea} className="ip-path ip-list" rows={4} {...NO_AUTO} value={paths} placeholder={suggestions.folders.length ? suggestions.folders.slice(0, 3).join("\n") : "assets/models\ndata/captures"} aria-label="Folders relative to the project, one per line" disabled={busy} onChange={event => setPaths(event.currentTarget.value)} />
    {!!suggestions.folders.length && <div className="dchips ig-picks" role="group" aria-label="Folders in .gitignore">
      {(all ? suggestions.folders : suggestions.folders.slice(0, SHOWN)).map(folder => <ChipToggle key={folder} label={folder} icon={listed.includes(folder) ? "check" : "plus"} selected={listed.includes(folder)} onToggle={() => toggle(folder)} />)}
      {!all && suggestions.folders.length > SHOWN && <Button look="quiet" onClick={() => setAll(true)}>Show {suggestions.folders.length - SHOWN} more</Button>}
    </div>}
    {!!suggestions.files.length && <div className="am-sub">Files stay on each machine: {suggestions.files.join(", ")}</div>}
    <div className="am-sub">Each folder must be wholly ignored by Git and contain no tracked or staged files. These folders use file sync, including conflict copies. Other uncommitted edits stay local. Build output, linked repositories and paths excluded in .beepignore stay local.</div>
    <div className="am-sub">Beep waits while Syncthing manages these folders. Turning this off leaves local files in place.</div>
    <div className="am-msg bad" role="status" hidden={!error}>{error}</div><div className="am-line ip-acts"><Button look="quiet" onClick={cancel}>Cancel</Button><Button look="primary" disabled={busy} onClick={() => save(enabled, paths, setError, setBusy)}>{busy ? "Saving…" : "Save"}</Button></div></>;
}

/// Edits the project's sync rules, which every machine applies after its `.beepignore`.
export function editSyncRules(project: string, initial: string, onChanged: () => void) {
  let busy = false;
  const { close, render } = openSheet("Sync rules", "am-sheet", { canClose: () => !busy });
  const save = async (rules: string, setError: (message: string) => void, setBusy: (value: boolean) => void) => {
    if (busy) return;
    busy = true;
    setBusy(true);
    try {
      await invoke("project_sync_rules", { project, rules });
      busy = false;
      close();
      onChanged();
    } catch (error) {
      busy = false;
      setError(reason(error));
      setBusy(false);
    }
  };
  flushSync(() => render(<SyncRulesView initial={initial} save={save} cancel={() => { if (!busy) close(); }} />));
}

function SyncRulesView({ initial, save, cancel }: { initial: string; save(rules: string, setError: (message: string) => void, setBusy: (value: boolean) => void): void; cancel(): void }) {
  const [rules, setRules] = useState(initial), [busy, setBusy] = useState(false), [error, setError] = useState("");
  const textarea = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { textarea.current?.focus(); }, []);
  return <><SheetHead title="Sync rules" />
    <div className="am-label">Rules in .gitignore syntax, one per line</div><textarea ref={textarea} className="ip-path ip-list" rows={8} {...NO_AUTO} value={rules} placeholder={"datasets/\n*.log\n!target/release/app"} aria-label="Rules in .gitignore syntax, one per line" disabled={busy} onChange={event => setRules(event.currentTarget.value)} />
    <div className="am-sub">Every machine leaves out matching paths, after the project's .beepignore. A ! rule keeps a path the defaults leave out, such as build output.</div>
    <div className="am-msg bad" role="status" hidden={!error}>{error}</div><div className="am-line ip-acts"><Button look="quiet" onClick={cancel}>Cancel</Button><Button look="primary" disabled={busy} onClick={() => save(rules, setError, setBusy)}>{busy ? "Saving…" : "Save"}</Button></div></>;
}
