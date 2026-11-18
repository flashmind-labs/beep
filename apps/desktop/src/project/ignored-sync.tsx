import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { openSheet } from "../ui/parts.tsx";
import { invoke } from "../ui/tauri.ts";
import { reason } from "../ui/format.ts";
import { gitignoreSuggestions, ignoredSelection, listedFolders, type GitignoreSuggestions, type IgnoredSync } from "./ignored-sync-state.ts";
import { Button, Checkbox, ChipToggle, SheetHead } from "../ui/kit.tsx";
import { readFile } from "../agentpane/read.ts";
import type { ProjectPlace } from "./projects-state.ts";

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
  const suggested = Promise.any(places.map(place => readFile(place.id, `${place.folder}/.gitignore`, null, () => {})))
    .then(file => gitignoreSuggestions(new TextDecoder().decode(file.bytes)), () => ({ folders: [], files: [] }));
  flushSync(() => render(<IgnoredSyncView initial={initial} suggested={suggested} save={save} cancel={() => { if (!busy) close(); }} />));
}

function IgnoredSyncView({ initial, suggested, save, cancel }: { initial: IgnoredSync; suggested: Promise<GitignoreSuggestions>; save(enabled: boolean, paths: string, setError: (message: string) => void, setBusy: (value: boolean) => void): void; cancel(): void }) {
  const [enabled, setEnabled] = useState(initial.enabled), [paths, setPaths] = useState(initial.folders.join("\n"));
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const [suggestions, setSuggestions] = useState<GitignoreSuggestions>({ folders: [], files: [] });
  const textarea = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => { textarea.current?.focus(); }, []);
  useEffect(() => { suggested.then(setSuggestions); }, [suggested]);
  const listed = listedFolders(paths);
  const toggle = (folder: string) => setPaths(listed.includes(folder) ? listed.filter(p => p !== folder).join("\n") : [...listed, folder].join("\n"));
  return <><SheetHead title="Sync gitignored" />
    <label className="am-line"><Checkbox checked={enabled} disabled={busy} change={setEnabled} />Sync selected ignored folders on all machines</label>
    <div className="am-label">Folders relative to the project, one per line</div><textarea ref={textarea} className="ip-path ip-list" rows={4} value={paths} placeholder={suggestions.folders.length ? suggestions.folders.join("\n") : "assets/models\ndata/captures"} aria-label="Folders relative to the project, one per line" disabled={busy} onChange={event => setPaths(event.currentTarget.value)} />
    {!!(suggestions.folders.length || suggestions.files.length) && <div className="dchips" aria-label="In .gitignore">
      {suggestions.folders.map(folder => <ChipToggle key={folder} label={folder} selected={listed.includes(folder)} onToggle={() => toggle(folder)} />)}
      {suggestions.files.map(file => <ChipToggle key={file} label={file} selected={false} disabled title="Only folders sync for now; files stay on each machine" onToggle={() => {}} />)}
    </div>}
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
    <div className="am-label">Rules in .gitignore syntax, one per line</div><textarea ref={textarea} className="ip-path ip-list" rows={8} value={rules} placeholder={"datasets/\n*.log\n!target/release/app"} aria-label="Rules in .gitignore syntax, one per line" disabled={busy} onChange={event => setRules(event.currentTarget.value)} />
    <div className="am-sub">Every machine leaves out matching paths, after the project's .beepignore. A ! rule keeps a path the defaults leave out, such as build output.</div>
    <div className="am-msg bad" role="status" hidden={!error}>{error}</div><div className="am-line ip-acts"><Button look="quiet" onClick={cancel}>Cancel</Button><Button look="primary" disabled={busy} onClick={() => save(rules, setError, setBusy)}>{busy ? "Saving…" : "Save"}</Button></div></>;
}
