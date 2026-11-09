import { createElement, type MouseEvent as ReactMouseEvent } from "react";
import { flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { NewProjectView } from "./newproject-view.tsx";
// The "New project" sheet: a name, where the repository goes (each host with
// a token, or Local only), its owner and visibility, and the machines that
// get a checkout. The server creates it, and each machine clones it.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { openMenu } from "../ui/menu.ts";
import { cleanError } from "./importproject-state.ts";
import { canCreate, pickHost, startForm, toggleMachine } from "./newproject-state.ts";
import type { Created, Forge, NewProjectForm } from "./newproject-state.ts";
import type { ProjectRow } from "../project/projects-state.ts";

/// What the window hands the sheet.
export interface NewProjectOptions {
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

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, opts: NewProjectOptions = {}, close: (() => void) | null = null;
/// undefined while read, null from a server too old to make projects.
let forges: Forge[] | null | undefined;
let loadError: string | null = null;
let form: NewProjectForm = startForm([], undefined);
let creating = false, error: string | null = null, created: Created | null = null;
/// Which opening of the sheet this is: a create still running when it closed
/// reports to no later one.
let opening = 0;
let view: DocumentView | null = null;

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
  ({ root, sheet, close } = openSheet("New project", "am-sheet ip-sheet np-sheet", { onClose: closed }));
  view = createDocumentView(sheet);
  draw();
  const mine = opening;
  invoke<Forge[] | null>("project_forges")
    .then(got => {
      if (mine !== opening || !root) return;
      forges = got;
      form = { ...startForm(got || [], form.machines[0]), name: form.name };
    }, e => { if (mine === opening && root) { forges = []; loadError = cleanError(String(e)); } })
    .finally(() => { if (mine === opening && root) draw(); });
}

/// Closes the sheet; a project made by then opens. One still being made
/// opens once it is in.
export function closeNewProject() {
  close?.();
}

function closed() {
  view?.unmount(); view = null;
  root = sheet = close = null;
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
    const made = await invoke<Created>("create_project", { name: form.name.trim(), repository, machineIds: form.machines });

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

const hostError = () => (forges || []).find(f => f.host === form.host && !f.owners.length)?.error ?? null;
function setForm(next: NewProjectForm) { form = next; error = null; draw(); }
function draw() {
  if (!view) return;
  const current = view;
  flushSync(() => current.render(createElement(NewProjectView, {
    forges, loadError, form, machines: online(), creating, error, created, hostError: hostError(),
    name: (name: string) => setForm({ ...form, name }),
    host: (host: string | null) => setForm(pickHost(form, forges ?? [], host)),
    owner: (event: ReactMouseEvent<HTMLButtonElement>) => {
      const anchor = event.currentTarget;
      openMenu(event.nativeEvent, "Owner", (forges ?? []).find(f => f.host === form.host)?.owners.map(owner => ({ label: owner, on: () => { setForm({ ...form, owner }); if (anchor.isConnected) anchor.focus(); } })) ?? []);
    },
    privacy: (isPrivate: boolean) => setForm({ ...form, isPrivate }),
    machine: (id: string) => setForm(toggleMachine(form, id)), submit: () => void submit(),
    back: () => { closeNewProject(); opts.onBack?.(); }, close: closeNewProject,
    clone: (url: string) => { closeNewProject(); opts.onClone?.(url); },
  })));
}
