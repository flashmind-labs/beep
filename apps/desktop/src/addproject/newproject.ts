// The "New project" sheet: a name, where the repository goes (each host with
// a token, or Local only), its owner and visibility, and the machines that
// get a checkout. The server creates it, and each machine clones it.
import { invoke } from "../ui/tauri.ts";
import { focusBack, sheetShell } from "../ui/parts.ts";
import { button, el } from "../ui/dom.ts";
import { openMenu } from "../ui/menu.ts";
import { cleanError, present } from "./importproject-state.ts";
import { canCreate, creatingText, goesText, landedLines, pickHost, shownName, startForm, toggleMachine } from "./newproject-state.ts";
import type { Created, Forge, NewProjectForm } from "./newproject-state.ts";
import type { ProjectRow } from "../project/projects-state.ts";

/// What the window hands the sheet.
export interface NewProjectOptions {
  /// The machines Home lists.
  machines?: readonly { id: string; name: string; online: boolean }[];
  /// Runs with where it landed first, once the sheet closes on it.
  onDone?: (project: ProjectRow) => void;
  /// Back to the Add a project sheet.
  onBack?: () => void;
  /// Opens the Import a project sheet with the repository filled in, when no
  /// machine got it.
  onClone?: (repo: string) => void;
}

let root: HTMLDivElement | null = null, sheet: HTMLDivElement | null = null, opts: NewProjectOptions = {}, back: Element | null = null;
/// undefined while read, null from a server too old to make projects.
let forges: Forge[] | null | undefined;
let loadError: string | null = null;
let form: NewProjectForm = startForm([], undefined);
let creating = false, error: string | null = null, created: Created | null = null;
/// Which opening of the sheet this is: a create still running when it closed
/// reports to no later one.
let opening = 0;

const online = () => (opts.machines || []).filter(m => m.online);
const picked = () => form.machines.map(id => online().find(m => m.id === id)?.name ?? id);

export function openNewProject(options: NewProjectOptions = {}) {
  if (root) return;
  opening++;
  opts = options;
  back = document.activeElement;
  forges = undefined;
  loadError = null;
  form = startForm([], online()[0]?.id);
  creating = false;
  error = null;
  created = null;
  ({ root, sheet } = sheetShell("New project", "am-sheet ip-sheet"));
  root.addEventListener("mousedown", e => { if (e.target === root) closeNewProject(); });
  document.body.append(root);
  document.addEventListener("keydown", onKey, true);
  draw();
  invoke<Forge[] | null>("project_forges")
    .then(got => {
      forges = got;
      form = { ...startForm(got || [], form.machines[0]), name: form.name };
    }, e => { forges = []; loadError = cleanError(String(e)); })
    .finally(() => { if (root) draw(); });
}

/// Closes the sheet; a project made by then opens. One still being made
/// opens once it is in.
export function closeNewProject() {
  if (!root) return;
  document.removeEventListener("keydown", onKey, true);
  root.remove();
  root = sheet = null;
  focusBack(back);
  if (created?.imported[0]) opts.onDone?.(created.imported[0]);
}

function onKey(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  e.stopPropagation();
  closeNewProject();
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

function draw() {
  if (!sheet) return;
  const body = forges === undefined ? [el("div", "am-wait", el("span", "dspin"))]
    : forges === null ? [el("div", "am-sub", "The server needs an update before it creates projects."), foot(null)]
    : creating ? [el("div", "am-wait", el("span", "dspin"), creatingText(form, picked()))]
    : created ? done(created)
    : fields();
  sheet.replaceChildren(el("div", "am-head", el("b", "", "New project"), el("kbd", "", "esc")), ...present(body));
  const name = sheet.querySelector<HTMLInputElement>(".ip-field");
  if (name) { name.focus(); name.setSelectionRange(name.value.length, name.value.length); }
}

/// Keeps the name's focus while typing.
function syncTyped() {
  const go = sheet!.querySelector<HTMLButtonElement>(".ip-go");
  if (go) go.disabled = !canCreate(form) || !!hostError();
  sheet!.querySelector(".am-msg.bad")?.remove();
  error = null;
}

function foot(go: HTMLButtonElement | null) {
  const again = button("Back", "dbtn quiet", () => { closeNewProject(); opts.onBack?.(); });
  return el("div", "am-line ip-foot", go, again);
}

const hostError = () => (forges || []).find(f => f.host === form.host && !f.owners.length)?.error ?? null;

function setForm(next: NewProjectForm) {
  form = next;
  error = null;
  draw();
}

function fields() {
  const list = forges || [];
  const input = el("input", "ip-field");
  input.type = "text";
  input.placeholder = "my-project";
  input.spellcheck = false;
  input.autocapitalize = "off";
  input.setAttribute("autocorrect", "off");
  input.setAttribute("aria-label", "Name");
  input.value = form.name;
  input.addEventListener("input", () => { form = { ...form, name: input.value }; syncTyped(); });
  input.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); submit(); } });
  const chip = (label: string, on: boolean, click: () => void) => {
    const b = button(label, "dchip", click);
    b.setAttribute("aria-pressed", String(on));
    return b;
  };
  const hosts = el("div", "dchips",
    ...list.map(f => chip(f.github ? "GitHub" : f.host, form.host === f.host, () => setForm(pickHost(form, list, f.host)))),
    chip("Local only", form.host === null, () => setForm(pickHost(form, list, null))));
  const owners = list.find(f => f.host === form.host)?.owners ?? [];
  const owner = button(form.owner, "dbtn", e => openMenu(e, "Owner", owners.map(o => ({ label: o, on: () => setForm({ ...form, owner: o }) }))));
  const machines = online();
  const go = button("Create", "dbtn pri ip-go", submit);
  go.disabled = !canCreate(form) || !!hostError();
  const note = goesText(form, picked());
  return [
    el("div", "am-label", "Name"),
    el("div", "am-line", input),
    el("div", "am-label", "Repository on"),
    el("div", "am-line", hosts),
    loadError ? el("div", "am-msg bad", loadError) : null,
    hostError() ? el("div", "am-msg bad", hostError()!) : null,
    form.host !== null && owners.length ? el("div", "am-label", "Owner and visibility") : null,
    form.host !== null && owners.length ? el("div", "am-line", owner, el("div", "dchips",
      chip("Private", form.isPrivate, () => setForm({ ...form, isPrivate: true })),
      chip("Public", !form.isPrivate, () => setForm({ ...form, isPrivate: false })))) : null,
    el("div", "am-label", "Machines"),
    machines.length
      ? el("div", "am-line", el("div", "dchips", ...machines.map(m => chip(m.name, form.machines.includes(m.id), () => setForm(toggleMachine(form, m.id))))))
      : el("div", "am-sub", "None of your machines is online."),
    note ? el("div", "am-sub ip-to", note) : null,
    error ? el("div", "am-msg bad", error) : null,
    foot(go),
  ];
}

function done(made: Created) {
  const first = made.imported[0];
  const failures = made.failed.map(f => el("div", "am-msg bad", `${f.machine_name}: ${f.why}`));
  return [
    el("div", "am-done", el("span", "ddot"), el("b", "", `${shownName(form)} created`)),
    ...(first ? landedLines(made.imported).map(line => el("div", "am-sub", line)) : []),
    ...failures,
    first
      ? button("Open project", "dbtn pri", closeNewProject)
      : el("div", "am-line",
        button("Clone…", "dbtn pri", () => { closeNewProject(); opts.onClone?.(made.url); }),
        button("Close", "dbtn quiet", closeNewProject)),
  ];
}
