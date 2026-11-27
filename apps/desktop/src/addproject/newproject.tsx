import { type MouseEvent as ReactMouseEvent, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { NewProjectView } from "./newproject-view.tsx";
// The "New project" sheet: a name, where the repository goes (each host with
// a token, or Local only), its owner and visibility, and the machines that
// get a checkout. Beep creates it, and each machine clones it.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.tsx";
import { openMenu } from "../ui/menu.tsx";
import { cleanError } from "./importproject-state.ts";
import { canCreate, pickHost, projectFolders, startForm, toggleMachine } from "./newproject-state.ts";
import { projectsIn } from "./importproject-state.ts";
import type { Created, Forge, NewProjectForm } from "./newproject-state.ts";
import type { ProjectRow } from "../project/projects-state.ts";

/// What the window hands the sheet.
interface NewProjectOptions {
  /// The machines Home lists.
  machines?: readonly { id: string; name: string; online: boolean; can_host?: boolean }[];
  /// Runs with where it landed first, once the sheet closes on it.
  onDone?: (project: ProjectRow) => void;
  /// Back to the Add a project sheet.
  onBack?: () => void;
  /// Opens the Import a project sheet with the repository filled in, when no
  /// machine got it.
  onClone?: (repo: string) => void;
}

let root: HTMLDivElement | null = null, opts: NewProjectOptions = {}, close: (() => void) | null = null;
/// undefined while read.
let forges: Forge[] | undefined;
let loadError: string | null = null;
let form: NewProjectForm = startForm([], undefined);
let creating = false, error: string | null = null, created: Created | null = null;
/// Which opening of the sheet this is: a create still running when it closed
/// reports to no later one.
let opening = 0;
let view: ((content: ReactNode) => void) | null = null;
/// Each machine's Projects in folder, by id, once read.
const defaults = new Map<string, string>();

const online = () => (opts.machines || []).filter(m => m.online);

export function openNewProject(options: NewProjectOptions = {}) {
  if (root) return;
  opening++;
  opts = { ...options, machines: options.machines?.filter(m => m.can_host !== false) };
  forges = undefined;
  loadError = null;
  form = startForm([], online()[0]?.id);
  creating = false;
  error = null;
  created = null;
  defaults.clear();
  ({ root, close, render: view } = openSheet("New project", "am-sheet ip-sheet np-sheet", { onClose: closed }));
  draw();
  const mine = opening;
  for (const machine of online()) {
    void projectsIn(invoke, machine.id).then(folder => {
      if (mine !== opening || !root) return;
      defaults.set(machine.id, folder);
      draw();
    });
  }
  invoke<Forge[]>("project_forges")
    .then(got => {
      if (mine !== opening || !root) return;
      forges = got;
      form = { ...startForm(got, form.machines[0]), name: form.name };
    }, e => { if (mine === opening && root) { forges = []; loadError = cleanError(String(e)); } })
    .finally(() => { if (mine === opening && root) draw(); });
}

/// Closes the sheet; a project made by then opens. One still being made
/// opens once it is in.
function closeNewProject() {
  close?.();
}

function closed() {
  view = null;
  root = close = null;
  if (created?.imported[0]) opts.onDone?.(created.imported[0]);
}

async function submit() {
  if (creating || !canCreate(form)) return;
  creating = true;
  error = null;
  draw();
  const mine = opening, then = opts.onDone;
  const repository = form.host === null ? null : { host: form.host, owner: form.owner, private: form.isPrivate };

  try {
    const made = await invoke<Created>("create_project", { name: form.name.trim(), repository, machineIds: form.machines, folders: projectFolders(form, defaults) });

    if (mine !== opening || !root) {
      if (made.imported[0]) then?.(made.imported[0]);
      return;
    }

    created = made;
  } catch (e) {
    if (mine !== opening || !root) {
      console.warn("creating a project:", e);
      return;
    }

    error = cleanError(String(e));
  }

  creating = false;
  if (root) draw();
}

const hostError = () => (forges ?? []).find(f => f.host === form.host && !f.owners.length)?.error ?? null;
function setForm(next: NewProjectForm) { form = next; error = null; draw(); }
function draw() {
  if (!view) return;
  const current = view;
  flushSync(() => current(<NewProjectView
    forges={forges} loadError={loadError} form={form} machines={online()} defaults={defaults} creating={creating} error={error} created={created} hostError={hostError()}
    name={(name: string) => setForm({ ...form, name })}
    host={(host: string | null) => setForm(pickHost(form, forges ?? [], host))}
    owner={(event: ReactMouseEvent<HTMLButtonElement>) => {
      const anchor = event.currentTarget;
      openMenu(event.nativeEvent, "Owner", (forges ?? []).find(f => f.host === form.host)?.owners.map(owner => ({ label: owner, on: () => { setForm({ ...form, owner }); if (anchor.isConnected) anchor.focus(); } })) ?? []);
    }}
    privacy={(isPrivate: boolean) => setForm({ ...form, isPrivate })}
    machine={(id: string) => setForm(toggleMachine(form, id))} submit={() => void submit()}
    folder={(id: string, folder: string) => setForm({ ...form, folders: { ...form.folders, [id]: folder } })}
    back={() => { closeNewProject(); opts.onBack?.(); }} close={closeNewProject}
    clone={(url: string) => { closeNewProject(); opts.onClone?.(url); }}
  />));
}
