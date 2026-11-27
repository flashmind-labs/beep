import { flushSync } from "react-dom";
import { createDocumentScope } from "../../ui/document-root.tsx";
import { MachineList, MachineLoading } from "./list-view.tsx";
import { MachineDetail, type MachineLog } from "./detail-view.tsx";
import { subscribeChanges, subscribeEach, topic } from "../../ui/mesh-events.ts";
import { pushedList } from "../../ui/snapshot-state.ts";
import { AMBIENT_MS, SLOW_MS } from "../../ui/active.ts";
// machines/: the Machines list, Home's machine table (table.ts) and the
// Add a machine sheet. Entry: this file.
import { openAddMachine } from "./addmachine.tsx";
import { ensureMachines, MACHINES, removeListed, renameListed, takeTraffic } from "../../stores/machines/machine-store.ts";
import { openJoinTicket } from "./jointicket.tsx";
import { errorText, offeredNode, type NodeUpdate, updatedTo } from "../../stores/machines/machines-state.ts";
import { S, setView } from "../../stores/bar/core.ts";
import { type BarPage, type BarPageSlot, openBarPage } from "../../stores/bar/bar-page.ts";
import { hintModel } from "../../stores/bar/hints.ts";
import { notice } from "../../parts/session/viewer-overlay.tsx";
import { invoke, listen } from "../../ui/tauri.ts";
import { loadClocks } from "../../stores/machines/clock.ts";
import { openSignIn } from "./signin.tsx";
import { type AdapterInstall } from "../../stores/machines/adapters-state.ts";
import { savedIn } from "../../ui/platform.ts";
import { renameSheet } from "../../ui/menu.tsx";
import type { MachineView } from "../../parts/machines/machine-state.ts";

// ---------- Machines ----------

export type { MachineEnv } from "../../parts/machines/machine-state.ts";

/// A machine or phone in the mesh (MachineView).
export interface Machine extends MachineView {
  /// The agents whose ACP adapter it is installing or could not install.
  adapters?: AdapterInstall[];
}

/// The machine whose Remove waits for an answer.
export interface Asking { id: string; busy: boolean; error: string }

export interface MachineSearch { id: string; folder: string | null; draft: string | null; busy: boolean; error: string }

// The machines running beep-node (ListMachines), from Settings; `asking`
// the machine whose Remove waits for an answer.
const MC: {
  error: string;
  asking: Asking | null;
  /// The newest beep-node release, null until read or while offline.
  released: string | null;
  /// Update's progress by machine.
  updates: Map<string, NodeUpdate>;
  /// Tools behind their newest release (ToolUpdate).
  tools: ToolUpdate[];
  /// A tool's Update by "<machine> <tool>": installing, or why it failed.
  toolUps: Map<string, { busy: boolean; error: string }>;
  /// The version each Update installed this session, by "<machine> <tool>".
  installed: Map<string, string>;
  /// Try again on a machine's adapters: asking, or why it failed.
  adapterTries: Map<string, { busy: boolean; error: string }>;
} = { error: "", asking: null, released: null, updates: new Map(), tools: [], toolUps: new Map(), installed: new Map(), adapterTries: new Map() };

/// Where the open machine looks for projects (GetMachineSearch): null until
/// read, or when its beep-node cannot say; `draft` is the field while
/// changing it.
let search: MachineSearch | null = null;
let searchEditGeneration = 0;

/// The open machine's log (GetMachineLog): `text` is null until read.
let log: MachineLog | null = null;

/// A tool on a machine with a newer release (ToolUpdate).
export interface ToolUpdate {
  machine_id: string;
  tool: string;
  version: string;
  latest: string;
}

// ---------- Where Machines draws ----------

/// The window's Settings sheet shows Machines in its right pane: `root` is
/// the box it draws into instead of the page, `machine` the machine open
/// there (null for the list), and `opened` tells Settings when a click
/// opens one or goes back, so its breadcrumb follows.
interface MachinesHost {
  root: HTMLElement;
  machine: string | null;
  opened(id: string | null): void;
  /// The list changed: a machine came or went.
  changed(): void;
}
let host: MachinesHost | null = null;

/// Draws Machines into `root` from now on, until `machinesOut` or until
/// `root` leaves the page. The page's own view (`S.view`) is left alone.
export function machinesIn(root: HTMLElement, opened: MachinesHost["opened"], changed: MachinesHost["changed"]) {
  host = { root, machine: null, opened, changed };
}
export function machinesOut() {
  if (host) embedded.remove(["machines"]);
  host = null;
}
const hosted = () => (host?.root.isConnected ? host : null);
/// Whether Machines is on screen, in Settings or on the page.
const shown = () => !!hosted() || S.view === "machines";

/// The list, or in Settings machine `id` on its own page.
export async function renderMachines(id: string | null = null) {
  const at = hosted();

  if (at) {
    at.machine = id;
  } else {
    setView("machines");
  }

  MC.asking = null;
  if (log?.id !== id) log = null;
  drawMachines();
  await readList();
  if (id && MACHINES.list?.some(m => m.id === id && m.can_host !== false)) void loadSearch(id);
  if (id && MACHINES.list?.some(m => m.id === id && m.can_host === false)) return;
  // Looking the newest releases up can take a while; the page shows first.
  void loadNodeRelease();
}

// Concurrent invalidations share one read and request a fresh pass before publishing.
let listRead: Promise<void> | null = null;
let listAgain = false;
function readList(): Promise<void> {
  if (listRead) { listAgain = true; return listRead; }
  listRead = (async () => {
    do {
      listAgain = false;
      try {
        await Promise.all([ensureMachines(), loadClocks(invoke)]);
        if (listAgain) continue;
        MC.error = "";
      } catch (e) {
        if (listAgain) continue;
        MC.error = String(e);
      }
      if (shown()) drawMachines();
    } while (listAgain);
  })();
  const read = listRead;
  void read.then(() => { if (listRead === read) listRead = null; }, () => { if (listRead === read) listRead = null; });
  return read;
}

/// Starts the machine updates once per webview, from its entry point.
export function followToolUpdates() {
  // Opened before the machines connected: the page follows them as they come.
  listen("reconnected", () => { if (shown()) void readList(); });
  // The list, the clocks and this computer's state come in the push, which the store takes first.
  subscribeChanges(topic("machines"), { deliver: "latest", receive: () => { if (shown()) drawMachines(); } }, { when: shown, wait: AMBIENT_MS });
  // The tools behind their newest release come in the `tools` push, which the machines ask their nodes for.
  subscribeEach(topic("tools"), event => {
    const pushed = pushedList<ToolUpdate>(event.data, "tool_updates");
    if (!pushed) return;
    if ("error" in pushed) { console.warn("tool updates:", pushed.error); return; }
    if (JSON.stringify(pushed.list) === JSON.stringify(MC.tools)) return;

    MC.tools = pushed.list;
    if (shown()) drawMachines();
  }, { when: shown });
  // What each machine moved comes with the telemetry; Home takes it the same way while it shows.
  subscribeChanges(topic("telemetry"), { deliver: "each", receive: event => { if (takeTraffic(event.data) && shown()) drawMachines(); } }, { when: shown, wait: SLOW_MS });
}

async function loadNodeRelease() {
  const released = await invoke<string | null>("node_release").catch(e => { console.warn("beep-node release:", e); return null; });
  if (released === MC.released) return;
  MC.released = released;
  if (shown()) drawMachines();
}

async function loadSearch(id: string) {
  search = { id, folder: null, draft: null, busy: false, error: "" };

  try {
    const folder = await invoke<string>("machine_search", { machineId: id });
    if (search?.id === id) search.folder = folder;
  } catch (e) {
    console.warn("search folder:", e);
  }

  if (shown()) drawMachines();
}

/// Has the open machine look for projects in `folder` and search again.
async function saveSearch(folder: string) {
  const at = search;
  if (!at || at.busy) return;
  const generation = searchEditGeneration;
  at.busy = true;
  at.draft = folder;
  at.error = "";
  drawMachines();

  try {
    at.folder = await invoke<string>("set_machine_search", { machineId: at.id, search: folder });
    if (generation === searchEditGeneration) at.draft = null;
  } catch (e) {
    if (generation === searchEditGeneration) at.error = errorText(e);
  }

  at.busy = false;
  if (shown()) drawMachines();
}

/// Reads machine `id`'s log.
async function readLog(id: string) {
  const at: MachineLog = log = { id, text: log?.id === id ? log.text : null, busy: true, error: "" };
  drawMachines();

  try {
    at.text = await invoke<string>("machine_log", { id });
  } catch (e) {
    at.error = errorText(e);
  }

  at.busy = false;
  if (shown()) drawMachines();
}

async function saveLog(m: Machine) {
  try {
    const path = await invoke<string>("save_machine_log", { name: m.name, text: log?.text ?? "" });
    notice(`Saved ${path.split("/").pop()} to ${savedIn(path)}`, { path });
  } catch (e) {
    notice(`Could not save it. ${e}`, { bad: true });
  }
}

/// Installs the newest release of `tool` on machine `id`.
async function updateTool(id: string, tool: string) {
  const key = `${id} ${tool}`;
  if (MC.toolUps.get(key)?.busy) return;
  MC.toolUps.set(key, { busy: true, error: "" });
  if (shown()) drawMachines();

  try {
    const to = await invoke<string>("update_machine_tool", { machineId: id, tool });
    MC.toolUps.delete(key);
    MC.installed.set(key, to);
    MC.tools = MC.tools.filter(u => !(u.machine_id === id && u.tool === tool));
    const found = MACHINES.list?.find(m => m.id === id)?.env?.tools.find(t => t.name === tool);
    if (found) found.version = to;
  } catch (e) {
    MC.toolUps.set(key, { busy: false, error: errorText(e) });
  }

  if (shown()) drawMachines();
}

/// Try again: the machine installs the adapters it could not, now.
async function retryAdapters(id: string) {
  if (MC.adapterTries.get(id)?.busy) return;
  MC.adapterTries.set(id, { busy: true, error: "" });
  if (shown()) drawMachines();

  try {
    await invoke("agent_adapters_install", { machineId: id });
    MC.adapterTries.delete(id);
    // The node reads them as installing at once; its report follows.
    for (const a of MACHINES.list?.find(m => m.id === id)?.adapters ?? []) a.failed = null;
  } catch (e) {
    MC.adapterTries.set(id, { busy: false, error: errorText(e) });
  }

  if (shown()) drawMachines();
}

/// Saves machine `id`'s new name, shown on every machine.
async function renameMachine(id: string, name: string) {
  await invoke("rename_machine", { id, name });
  renameListed(id, name);
  hosted()?.changed();
  if (shown()) drawMachines();
}

/// Unlinks the machine Remove asked about, online or not.
async function removeMachine() {
  const asking = MC.asking;
  if (!asking || asking.busy) return;
  asking.busy = true;
  asking.error = "";
  drawMachines();

  try {
    await invoke("remove_machine", { id: asking.id });
    removeListed(asking.id);
    MC.asking = null;
    hosted()?.changed();
  } catch (e) {
    asking.busy = false;
    asking.error = String(e);
  }

  if (shown()) drawMachines();
}

/// Has the server update machine `id`'s beep-node, then waits for it to
/// come back on the new version.
async function runUpdate(id: string) {
  MC.updates.set(id, { step: "updating" });
  if (shown()) drawMachines();

  try {
    const done = await invoke<{ from: string; to: string; restarting: boolean; note: string }>("update_machine", { id });

    if (done.restarting) {
      MC.updates.set(id, { step: "restarting", to: done.to, note: done.note });
      void waitForNode(id, done.to, done.note ? IDLE_POLLS : RESTART_POLLS);
    } else if (done.from === done.to) {
      MC.updates.set(id, { step: "note", text: `beep-node ${done.to} is the newest release.` });
    } else {
      MC.updates.set(id, { step: "note", text: `Installed beep-node ${done.to}. ${done.note}` });
    }
  } catch (e) {
    MC.updates.set(id, { step: "failed", error: errorText(e) });
  }

  if (shown()) drawMachines();
}

/// Restart deadlines, including extra time for running sessions to finish.
const RESTART_POLL_MS = 3000, RESTART_POLLS = 60, IDLE_POLLS = RESTART_POLLS + 600;

/// Follows restart reports until the requested version appears, with the existing deadline.
async function waitForNode(id: string, to: string, polls: number) {
  await new Promise<void>(resolve => {
    let reading = false, again = false, done = false;
    const finish = () => { done = true; clearTimeout(deadline); unfollow(); resolve(); };
    const check = async () => {
      if (reading) { again = true; return; }
      reading = true;

      do {
        again = false;
        const now = MC.updates.get(id);
        if (done) break;
        if (now?.step !== "restarting" || now.to !== to) { finish(); break; }

        try {
          const list = await ensureMachines();
          if (done) break;
          const machine = list.find(machine => machine.id === id);

          if (machine && updatedTo(machine, to)) {
            MC.updates.delete(id);
            if (shown()) drawMachines();
            finish();
          }
        } catch (error) { console.warn("machines while a node restarts:", error); }
      } while (again && !done);

      reading = false;
    };
    const unfollow = subscribeChanges(topic("machines"), { deliver: "latest", receive: () => check() });
    const deadline = setTimeout(() => {
      const now = MC.updates.get(id);

      if (now?.step === "restarting" && now.to === to) {
        MC.updates.set(id, { step: "failed", error: `It did not come back on beep-node ${to} yet. Check the machine.` });
        if (shown()) drawMachines();
      }

      finish();
    }, RESTART_POLL_MS * polls);
    void check();
  });
}

/// The newest beep-node released or on the account's machines, which the
/// others are behind.
// Only nodes that update themselves count: the Mac app's built-in node is
// whatever version the app was built with, which may never have been released.
const newest = () => offeredNode(MC.released, (MACHINES.list ?? []).filter(m => MACHINES.updatable.has(m.id)).map(m => m.version));

/// Machines in the Settings sheet's box. Its own scope keeps the bar window from loading the
/// command center's root.
const embedded = createDocumentScope<"machines">();

/// A Machines screen, drawn in the bar as its page or in the Settings sheet's box.
type MachinesPage = Extract<BarPage, { view: "machines" | "machine" | "machine-loading" }>;
let barPage: BarPageSlot | null = null;

function showScreen(at: MachinesHost | null, page: MachinesPage) {
  if (!at) {
    if (S.view !== "machines") return;
    const slot = barPage ??= openBarPage(() => { barPage = null; });
    flushSync(() => slot.show(page));
    return;
  }

  embedded.set([{ slot: "machines", host: at.root, content: page.view === "machines" ? <MachineList {...page.props} />
    : page.view === "machine" ? <MachineDetail {...page.props} /> : <MachineLoading /> }]);
}

function drawList(at: MachinesHost | null) {
  const refreshAfterSheet = () => { if (shown()) void renderMachines(hosted()?.machine ?? null).then(() => hosted()?.changed()); };
  showScreen(at, { view: "machines", props: {
    list: MACHINES.list, error: MC.error, hosted: !!at, mac: MACHINES.mac?.id ?? null, newest: newest(), asking: MC.asking,
    adapterTries: MC.adapterTries, hints: at ? null : hintModel([["esc", "back"]]), age: span,
    open: (id: string) => { if (at) { at.opened(id); void renderMachines(id); } },
    add: () => openAddMachine({ onClose: refreshAfterSheet }),
    link: () => openJoinTicket({ onClose: refreshAfterSheet }),
    askRemove: (id: string) => { MC.asking = { id, busy: false, error: "" }; drawMachines(); },
    cancelRemove: () => { MC.asking = null; drawMachines(); },
    remove: () => { void removeMachine(); },
    retryAdapter: (id: string) => { void retryAdapters(id); },
  } });
}

function drawDetail(at: MachinesHost | null, machine: Machine) {
  showScreen(at, { view: "machine", props: {
    machine, invoke, error: MC.error, mac: MACHINES.mac?.id ?? null, newest: newest(), updatable: MACHINES.updatable.has(machine.id),
    nodeUpdate: MC.updates.get(machine.id) ?? null, search, log, tools: MC.tools, toolUps: MC.toolUps,
    installed: MC.installed, traffic: MACHINES.traffic.find(t => t.id === machine.id), asking: MC.asking, age: span,
    searchEdit: () => { if (search) { search.draft = search.folder; drawMachines(); } },
    searchSave: (folder: string) => { void saveSearch(folder); }, searchCancel: stopEditing,
    signIn: (tool: string) => openSignIn(machine, tool, () => { if (shown()) void renderMachines(machine.id).then(() => hosted()?.changed()); }),
    updateNode: () => { void runUpdate(machine.id); }, updateTool: (tool: string) => { void updateTool(machine.id, tool); },
    logRead: () => { void readLog(machine.id); }, logSave: () => { void saveLog(machine); },
    rename: () => renameSheet({ title: `Rename ${machine.name}`, value: machine.name, run: name => renameMachine(machine.id, name) }),
    askRemove: () => { MC.asking = { id: machine.id, busy: false, error: "" }; drawMachines(); },
    remove: () => { void removeMachine(); }, cancelRemove: () => { MC.asking = null; drawMachines(); },
  } });
}

function drawMachines() {
  const at = hosted();
  const open = at?.machine ? MACHINES.list?.find(m => m.id === at.machine) : undefined;

  if (at?.machine && MACHINES.list && !open) {
    // Gone, such as just removed: back to the list.
    at.machine = null;
    at.opened(null);
  }

  if (!open && !at?.machine) {
    drawList(at);
    return;
  }

  if (open) {
    drawDetail(at, open);
  } else {
    showScreen(at, { view: "machine-loading" });
  }

}

function stopEditing() {
  if (!search) return;
  searchEditGeneration++;
  search.draft = null;
  search.error = "";
  drawMachines();
}

/// How long `ms` is, roughly: "5 min", "3 h", "2 d".
function span(ms: number) {
  const min = Math.max(1, Math.round(Math.abs(ms) / 60000));
  return min < 60 ? `${min} min` : min < 48 * 60 ? `${Math.round(min / 60)} h` : `${Math.round(min / 1440)} d`;
}
