// What the "New project" sheet decides, as pure functions so they can be
// tested without Tauri. newproject.ts draws it.
import type { ProjectRow } from "../project/projects-state.ts";

/// A git host the user has a token for, as `project_forges` lists it.
export interface Forge {
  host: string;
  github: boolean;
  owners: string[];
  error: string | null;
}

/// What `create_project` did.
export interface Created {
  url: string;
  imported: ProjectRow[];
  failed: { machine_id: string; machine_name: string; why: string }[];
}

export interface NewProjectForm {
  name: string;
  /// `Forge.host`; null for Local only.
  host: string | null;
  owner: string;
  isPrivate: boolean;
  machines: string[];
  /// The folder typed for a machine, by id; one not here takes its default.
  folders: Readonly<Record<string, string>>;
}

/// Where `create_project` puts the project on one machine: under `folder`.
export interface ProjectFolder {
  machine_id: string;
  folder: string;
}

/// The form once the hosts are read: the first host that answered, else
/// Local only, on `machine` when there is one.
export function startForm(forges: readonly Forge[], machine: string | undefined): NewProjectForm {
  const form: NewProjectForm = { name: "", host: null, owner: "", isPrivate: true, machines: machine ? [machine] : [], folders: {} };
  const first = forges.find(f => f.owners.length);
  return first ? pickHost(form, forges, first.host) : form;
}

/// Picks a host, or Local only with null, which keeps one machine.
export function pickHost(form: NewProjectForm, forges: readonly Forge[], host: string | null): NewProjectForm {
  const forge = forges.find(f => f.host === host);
  return {
    ...form,
    host,
    owner: forge?.owners[0] ?? "",
    machines: host === null ? form.machines.slice(0, 1) : form.machines,
  };
}

/// A machine chip: several on a host, one for Local only.
export function toggleMachine(form: NewProjectForm, id: string): NewProjectForm {
  const on = form.machines.includes(id);
  if (form.host === null) return { ...form, machines: on ? [] : [id] };
  return { ...form, machines: on ? form.machines.filter(m => m !== id) : [...form.machines, id] };
}

/// The folder the project goes under on `machine`: the one typed, else its
/// default.
export function folderOn(form: NewProjectForm, machine: string, defaults: ReadonlyMap<string, string>) {
  return form.folders[machine] ?? defaults.get(machine) ?? "";
}

/// The folder for each machine picked, as `create_project` takes them.
export function projectFolders(form: NewProjectForm, defaults: ReadonlyMap<string, string>): ProjectFolder[] {
  return form.machines.map(machine_id => ({ machine_id, folder: folderOn(form, machine_id, defaults).trim() }));
}

/// What hosts take as a name. Beep checks again.
export function validName(name: string) {
  const t = name.trim();
  return t.length <= 100 && /^[A-Za-z0-9_][A-Za-z0-9_.-]*$/.test(t) && !t.endsWith(".git");
}

export function canCreate(form: NewProjectForm) {
  return validName(form.name) && form.machines.length > 0 && (form.host === null || !!form.owner);
}

/// "owner/name", or the name alone for Local only.
export function shownName(form: NewProjectForm) {
  const name = form.name.trim();
  return form.host === null ? name : `${form.owner}/${name}`;
}

/// "a", "a and b", "a, b and c".
function joined(names: readonly string[]) {
  if (names.length < 2) return names[0] ?? "";
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/// What shows while it is made.
export function creatingText(form: NewProjectForm, machineNames: readonly string[]) {
  const onto = joined(machineNames);
  if (form.host === null) return `Creating ${shownName(form)} on ${onto}…`;
  return `Creating ${shownName(form)}, then cloning onto ${onto}…`;
}

/// The note above Create.
export function goesText(form: NewProjectForm) {
  return form.host !== null && form.machines.length ? "Each machine clones it and keeps it in sync." : "";
}

/// What follows a machine's folder: `/<name>`.
export function folderTail(form: NewProjectForm) {
  return `/${form.name.trim() || "my-project"}`;
}

/// Where it landed: one line when the folders are the same, else one per machine.
export function landedLines(imported: readonly Pick<ProjectRow, "machine_name" | "folder">[]) {
  const folders = new Set(imported.map(p => p.folder));
  if (folders.size === 1) return [`On ${joined(imported.map(p => p.machine_name))} in ${imported[0].folder}`];
  return imported.map(p => `On ${p.machine_name} in ${p.folder}`);
}
