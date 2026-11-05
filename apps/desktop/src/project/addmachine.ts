// The "Add <project> to <machine>" sheet of a project's "Where it lives": the
// folder there, then how far Beep's sync has brought the files.
import { invoke } from "../ui/tauri.ts";
import { openSheet } from "../ui/parts.ts";
import { button, el } from "../ui/dom.ts";
import { reason } from "../ui/format.ts";
import { folderBrowser } from "../addproject/folderbrowser.ts";
import { lastImport } from "../addproject/importproject.ts";
import { defaultParent } from "../addproject/importproject-state.ts";
import { browseStart, copyLine, destination, shortName, type CopyLine } from "./addmachine-state.ts";
import type { SyncDetail } from "./filesync.ts";
import type { MachineFolder } from "./projects-state.ts";

/// How often the sheet reads the sync while it copies.
const POLL_MS = 1000;

/// What the page hands the sheet.
export interface AddToMachine {
  project: string;
  machine: { id: string; name: string };
  /// `linux`, `macos` or `windows`, once the machine has said.
  os: string | null | undefined;
  /// This Mac's machine id: there Choose… opens the macOS folder picker.
  macId: string | null;
  /// The folder was made and the machine joined the sync.
  added: () => void;
}

/// Opens the sheet; each opening has its own state, and its reads stop when it closes.
export function openAddToMachine(o: AddToMachine) {
  const { project, machine } = o;
  const name = shortName(project);
  let folder = "", browsing: MachineFolder | null = null, reading = false, error: string | null = null;
  let adding = false, copying: CopyLine | null = null, open = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const { sheet, close } = openSheet(`Add ${name} to ${machine.name}`, "am-sheet ip-sheet ip-add", {
    onClose: () => { open = false; clearTimeout(timer); },
  });

  const draw = () => {
    if (!open) return;
    sheet.replaceChildren(el("div", "am-head", el("b", "", `Add ${name} to ${machine.name}`), el("kbd", "", "esc")), ...(copying ? copyBody(copying) : form()));
    if (!copying && !browsing) {
      const field = sheet.querySelector<HTMLInputElement>(".ip-dir");
      field?.focus();
      field?.setSelectionRange(field.value.length, field.value.length);
    }
  };

  /// Reads a folder on the machine (`path` null for where it starts).
  const read = async (path: string | null, then: (listing: MachineFolder) => void) => {
    if (reading) return;
    reading = true;
    draw();
    try {
      then(await invoke<MachineFolder>("list_machine_folder", { machineId: machine.id, path }));
      error = null;
    } catch (e) {
      error = reason(e);
    }
    reading = false;
    draw();
  };

  const choose = async () => {
    if (o.macId !== machine.id) {
      read(browseStart(folder), listing => { browsing = listing; });
      return;
    }
    try {
      const path = await invoke<string | null>("pick_folder", { start: browseStart(folder) });
      if (path) read(path, listing => { folder = destination(listing, project); });
    } catch (e) {
      error = reason(e);
      draw();
    }
  };

  const add = async () => {
    if (adding || !folder.trim()) return;
    adding = true;
    error = null;
    draw();
    try {
      await invoke<string>("project_add_to_machine", { project, machineId: machine.id, folder });
      o.added();
      copying = copyLine(null, machine.id, machine.name);
      poll();
    } catch (e) {
      error = reason(e);
    }
    adding = false;
    draw();
  };

  /// Reads the sync until the machine has it all; the page's row goes on from there.
  const poll = async () => {
    try {
      const detail = await invoke<SyncDetail | null>("project_sync_detail", { project });
      copying = copyLine(detail, machine.id, machine.name);
    } catch (e) {
      console.warn("project sync:", e);
    }
    if (!open) return;
    if (copying?.done) { close(); return; }
    draw();
    timer = setTimeout(poll, POLL_MS);
  };

  function form() {
    const field = el("input", "ip-path ip-dir");
    field.type = "text";
    field.spellcheck = false;
    field.autocapitalize = "off";
    field.setAttribute("autocorrect", "off");
    field.setAttribute("aria-label", `Folder on ${machine.name}`);
    field.value = field.title = folder;
    const go = button(adding ? "Adding…" : "Add", "dbtn pri ip-go", add);
    go.disabled = adding || !folder.trim();
    field.addEventListener("input", () => {
      folder = field.title = field.value;
      go.disabled = adding || !folder.trim();
      if (error) { error = null; sheet.querySelector(".am-msg.bad")?.remove(); }
    });
    field.addEventListener("keydown", e => { if (e.key === "Enter") { e.preventDefault(); add(); } });
    const pick = button(reading ? "Reading…" : "Choose…", "dbtn quiet", choose);
    pick.disabled = reading || adding;
    return [
      el("div", "am-label", `Folder on ${machine.name}`),
      el("div", "am-line", el("div", "ip-folder", field), pick),
      ...(browsing ? [browser(browsing)] : []),
      ...(error ? [el("div", "am-msg bad", error)] : []),
      el("div", "am-line ip-acts", button("Cancel", "dbtn quiet", close), go),
    ];
  }

  function browser(listing: MachineFolder) {
    return folderBrowser(listing, {
      reading,
      note: `It goes in ${destination(listing, project)}.`,
      go: path => read(path, next => { browsing = next; }),
      cancel: () => { browsing = null; draw(); },
      use: () => { folder = destination(listing, project); browsing = null; draw(); },
    });
  }

  function copyBody(line: CopyLine) {
    return [
      el("div", "am-wait", el("span", "dspin"), line.text),
      ...(line.note ? [el("div", "am-sub", line.note)] : []),
      el("div", "am-line ip-acts", button("Close", "dbtn quiet", close)),
    ];
  }

  draw();
  // The field starts at the machine's default, unless the user got there first.
  read(null, listing => {
    const parent = defaultParent(machine.id, listing.path, o.os, lastImport());
    if (!folder) folder = `${parent}/${name}`;
  });
}
