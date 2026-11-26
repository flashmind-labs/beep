import { withMentions } from "../agentpane/files-state.ts";
import { keepProjectAccount, keepProjectPick, lastProjectPick, projectAccount, startAccount } from "./agentmodels.ts";
import { Spinner } from "../ui/kit.tsx";
import { createRef, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-page.ts";
import { startAttachments } from "./start-attachments.tsx";
import { makeStartComposer, type StartComposer } from "./start-composer.tsx";
import { menuPlaceFor } from "./mentionmenu.ts";
import { FolderChoice, NewAgentNotes } from "./start-composer-view.tsx";
import { ChoiceMenuView, type ChoiceAction, type ChoiceMenu, type ChoiceRow } from "./choice-menu-view.tsx";
import { AccountChoice, BaseChoice, BaseInput, FolderInput } from "./choice-extra-view.tsx";
import { go } from "../center/nav.ts";
import { renderAgents } from "./agents-list.ts";
import { AG, accountsOf, type AgentHost, ensureAgentHosts, loadAccountNames, onAgentHosts, sentImage, type SentImage, type Started } from "./agents-state.ts";
import { agentKind } from "./agent-kind.ts";
import { ASSISTANT_WORDS, TASK_WORDS, assistantHost, resolveAssistant } from "./assistant-state.ts";
import { loadAssistant, type AssistantPick } from "../settings/assistant.ts";
import { ensureSessions, sessions } from "./sessions.ts";
import { ensureMachines, MACHINES } from "../machines/machine-store.ts";
import { openSession } from "./open-state.ts";
import { sessionPicks, startedWith } from "./open-state.ts";
import { closedPicker, drawPicker as drawPickerIn, focusSearch, type Picked, pillContent } from "./agentpicker.tsx";
import { knownModels, modelsOn } from "./model-store.ts";
import { AGENT_ORDER, type BaseBranches, baseFor, baseRows, type CheckoutPick, type CheckoutPlan, DEFAULT_CHECKOUT, folderPill, modelId, harnessFor, harnessesOn, keepEffort, knownWorktrees, machineRows, makesBranch, startPlace, tildePath } from "./agentpick.ts";
import { type AgentReturn, PANE, S, setView } from "../bar/core.ts";
import { completed, type FolderMenu, isOpen, menuKey, menuOf, NO_MENU, splitTyped, suggest } from "../addproject/foldercomplete.ts";
import { folderOn, type MachineFolder } from "../project/projects-state.ts";
import { projectHarnesses, readProjectHarnesses } from "../project/project-harnesses.tsx";
import { addHosts, barCheckout, barPlace, nextBarMachine } from "../project/projectbar.ts";
import { composeMessage } from "../chat/compose.tsx";
import { invoke } from "../ui/tauri.ts";

// ----- Start form -----

// The project composer on its own page: a project's "Start an agent here",
// a review or reply, or a debug run in a machine's workspace.

/// The machine and folder a start form is preset to, as a project's
/// "Start an agent here" gives them.
export interface AgentPlace {
  machine?: string | null;
  cwd?: string | null;
  /// The project's checkout on each machine.
  places?: { machine: string; cwd: string }[];
  /// The project's name, for the heading.
  project?: string | null;
}

/// The machine last picked for each project ("" for none), so the form
/// opens on it next time.
const LAST_MACHINE = "beep.lastMachine.v1";

function lastMachines(): Record<string, string> {
  try {
    const kept = JSON.parse(localStorage.getItem(LAST_MACHINE) ?? "{}");
    return kept && typeof kept === "object" ? kept : {};
  } catch {
    return {};
  }
}

/// The machine last picked for `project`, or "".
export function lastMachine(project: string | null | undefined) {
  const id = lastMachines()[project ?? ""];
  return typeof id === "string" ? id : "";
}

export function keepMachine(project: string | null | undefined, machine: string) {
  if (!machine) return;

  try {
    localStorage.setItem(LAST_MACHINE, JSON.stringify({ ...lastMachines(), [project ?? ""]: machine }));
  } catch {
    // Only a convenience: the form then opens without a machine.
  }
}

/// The machine the assistant last ran on, so the next one starts there.
const ASSISTANT_MACHINE = "beep.assistantMachine.v1";

export const lastAssistantMachine = () => localStorage.getItem(ASSISTANT_MACHINE) ?? "";

function keepAssistantMachine(machine: string) {
  try {
    localStorage.setItem(ASSISTANT_MACHINE, machine);
  } catch {
    // Only a convenience: the assistant then starts where Auto puts it.
  }
}

type FormError = { kind: "prompt" } | { kind: "failed"; message: string };
type StartPhase = { kind: "editing"; error: FormError | null } | { kind: "preparing" } | { kind: "starting" };

interface StartView {
  composer: StartComposer;
  attachments: ReturnType<typeof startAttachments>;
  page: BarPageSlot;
  phase: StartPhase;
}

let startView: StartView | null = null;
const formBusy = () => startView !== null && startView.phase.kind !== "editing";

/// Each project's branches for the Base menu, by project: `undefined`
/// while asked, `null` when the server cannot list them.
const BRANCHES = new Map<string, BaseBranches | null | undefined>();

/// Reads `project`'s branches once per form, then redraws.
function loadBranches(project: string) {
  BRANCHES.set(project, undefined);
  invoke<BaseBranches | null>("project_branches", { project })
    .catch(() => null)
    .then(list => { BRANCHES.set(project, list); if (S.view === "agentnew" && AG.form.project === project) drawParts(); });
}

const formBranches = () => AG.form.project ? BRANCHES.get(AG.form.project) : null;
/// What was typed for a new assistant chat and not sent: the form starts from it again.
let assistantDraft = "";

/// The start form with `prompt`; `from` is where Esc goes back to.
/// `at` presets the machine and folder, as a project's "Start an agent here" does.
/// `assistant` starts the Home assistant: no project, the machine's home folder.
export function renderAgentNew(prompt = "", from: AgentReturn = "hide", at: AgentPlace | null = null, assistant = false) {
  if (assistant) prompt ||= assistantDraft;
  setView("agentnew");
  AG.back = from;
  AG.form = assistant
    ? { prompt, assistant, machine: "", machinePicked: false, agent: null, cwd: "", project: null, checkout: DEFAULT_CHECKOUT, base: null, baseTyping: false, effort: null, open: null, query: "", typing: false }
    : { prompt, assistant, machine: at?.machine || lastMachine(at?.project), machinePicked: false, agent: null, cwd: at?.cwd || "", places: at?.places, project: at?.project ?? null, checkout: DEFAULT_CHECKOUT, base: null, baseTyping: false, effort: null, open: null, query: "", typing: false };
  if (assistant) presetAssistant(); else restoreProjectPick();
  drawFrame();
  startView?.composer.input.focus();
  void ensureAgentHosts();
  void loadAccountNames().then(() => { if (S.view === "agentnew") drawParts(); });
  ensureSessions().then(() => { if (S.view === "agentnew") drawParts(); }).catch(() => {});
  if (!MACHINES.list) ensureMachines().then(() => { if (S.view === "agentnew") drawParts(); }).catch(() => {});
  if (at?.project) {
    const project = at.project;
    loadBranches(project);
    void readProjectHarnesses(project).then(() => { if (S.view === "agentnew" && AG.form.project === project) drawParts(); });
  }
}

const PROMPT_MISSING = "Say what the agent should do.";

/// Draws the form's page: the key hints under it give way to an open menu.
function showForm(page: BarPageSlot, c: StartComposer) {
  const f = AG.form;
  flushSync(() => page.show({ view: "agent-new", props: { pane: PANE, composer: c.form, project: f.project ?? null, menuOpen: !!f.open, openList: renderAgents } }));
}

function formHost() { return AG.hosts?.find(m => m.machine_id === AG.form.machine) ?? null; }
/// The agents the chosen machine offers; before one is picked, any machine's.
/// Only those the project allows.
function formAgents() {
  return harnessesOn(AG.hosts, AG.form.machine || null, projectHarnesses(AG.form.project ?? null));
}

onAgentHosts(() => { if (S.view === "agentnew") drawAgentNew(); });

/// Keys of the start form; true when one was taken.
export function agentNewKeys(e: KeyboardEvent, cmd: boolean) {
  if (S.view !== "agentnew") return false;
  if (cmd && e.key === "Enter") { e.preventDefault(); void startAgent(); return true; }
  if (cmd && e.shiftKey && e.key.toLowerCase() === "m") { e.preventDefault(); togglePicker(); return true; }
  return false;
}

/// Redraws the form, keeping the prompt as typed when it shows already.
export function drawAgentNew() {
  if (startView?.composer.box.isConnected) drawParts();
  else drawFrame();
}

/// The form's frame: heading, prompt and the parts `drawParts` fills. The
/// prompt is drawn once, so redrawing the rest keeps what is typed.
function drawFrame() {
  const f = AG.form;
  const c = makeStartComposer({
    ids: true,
    actions: {
      submit: () => { if (startView?.composer === c) void startAgent(); },
      harness: () => { if (!formBusy()) togglePicker(); },
      choice: toggleChoice, keyDown: switchMachine,
      files: files => attachments.take(files), images: files => attachments.add(files), removeLast: () => attachments.removeLast(),
      input: value => {
        const view = startView;
        if (view?.composer !== c) return;
        f.prompt = value;
        if (f.assistant) assistantDraft = value;
        if (view.phase.kind === "editing" && view.phase.error?.kind === "prompt") {
          view.phase = { kind: "editing", error: null };
          drawParts();
        }
      },
    },
    place: () => {
      const f = AG.form;
      return S.view === "agentnew" && f.machine && f.agent && f.cwd ? menuPlaceFor(f.machine, f.agent, f.cwd) : null;
    },
  });
  const attachments = startAttachments(c, text => {
    if (startView?.composer !== c || startView.phase.kind !== "editing") return;
    startView.phase = { kind: "editing", error: text ? { kind: "failed", message: text } : null };
    c.update({ error: text });
  }, () => { if (startView?.composer === c) drawParts(); });
  c.setText(f.prompt);
  const page = openBarPage(() => {
    if (startView?.composer === c) {
      if (!formBusy()) attachments.discard();
      startView = null;
    }
    closedPicker(c.pick);
    c.dispose();
    stopOutside();
  });
  startView = { composer: c, attachments, page, phase: { kind: "editing", error: null } };
  showForm(page, c);
  attachments.bindStash(() => f.project ?? null);
  stopOutside();
  document.addEventListener("click", closeOutside);
  drawParts();
}

/// A click outside the open menu or picker closes it.
function closeOutside(e: MouseEvent) {
  const f = AG.form;
  if (S.view !== "agentnew" || formBusy() || !f.open || !(e.target instanceof Element)) return;
  if (e.target.closest(".agc-pick, .agc-menu, .pill")) return;
  f.open = null;
  drawParts();
}

function stopOutside() { document.removeEventListener("click", closeOutside); }

/// Brings the form's choices in line with the machine picked, then draws
/// everything but the prompt.
function drawParts() {
  const view = startView;
  if (S.view !== "agentnew" || !view?.composer.box.isConnected) return;
  const { composer: c, page, attachments, phase } = view;
  const f = AG.form, hosts = AG.hosts ?? [];
  showForm(page, c);

  // With no machine picked, or one that no longer runs agents: the project's
  // first machine with a checkout and an agent, else the first with an agent.
  // The assistant's is the one it last ran on, else the first that offers its harness.
  if (AG.hosts && !formHost()) {
    const harness = f.assistant ? formPick()?.agent ?? null : null;
    const offering = harness ? assistantHost(addHosts(AG.hosts), lastAssistantMachine(), harness) : null;
    f.machine = offering?.machine_id || (f.places && barPlace(f.places, AG.hosts, "")?.machine_id) || addHosts(AG.hosts)[0]?.machine_id || "";
    if (f.machine) f.cwd = folderOn(f.cwd, f.machine, f.places);
  }

  const preferred = !f.modelFor ? formPick()?.agent : null;
  f.agent = harnessFor(preferred ?? f.agent, formAgents(), AGENT_ORDER);
  f.effort = keepEffort(f.agent, f.effort);
  syncModel();
  const none = !!AG.hosts && !addHosts(hosts).length;
  const error = phase.kind === "editing" ? phase.error : null;
  const missing = error?.kind === "prompt";
  const busy = phase.kind !== "editing";
  const host = formHost();
  const words = f.assistant ? ASSISTANT_WORDS : TASK_WORDS;
  c.update({
    invalid: missing, placeholder: missing ? PROMPT_MISSING : words.placeholder, words, error: missing ? PROMPT_MISSING : error?.message ?? "",
    project: f.project ?? null,
    sendDisabled: busy || attachments.reading || !hosts.length || !f.machine,
    sendTitle: !f.machine ? "Pick a machine first" : "Start (⌘↩)",
    busy, open: f.open ?? null,
    folderHidden: f.assistant || !!f.typing, folderTitle: f.cwd.trim() || host?.workspace || "The machine's workspace",
  });
  attachments.setBusy(busy);
  if (!busy) attachments.files.moveTo(f.machine, f.cwd.trim() || host?.workspace || "");
  // Text set from code (a stash restore, a start) fits once drawn.
  requestAnimationFrame(c.grow);
  drawPill(c, none);
  drawPicker(c);
  drawUnder(c);
  c.update({ notes: <NewAgentNotes waiting={!AG.hosts} none={none} noMachine={!f.machine && !!AG.hosts?.length} openAgents={openAgents} openMachines={openMachines} /> });
}

export function openMachines() { void import("../machines/machines.tsx").then(m => m.renderMachines()); }
/// Opens machine `id`'s page in Settings.
export function openMachine(id: string) { void import("../settings/sheet.ts").then(m => m.openMachineSettings(id)); }
export function openAgents() { go("providers"); }

function drawPill(c: StartComposer, none: boolean) {
  const f = AG.form;
  c.update({ harnessDisabled: none || !f.agent });
  c.setHarness(none || !f.agent ? "No agent" : pillContent(f.agent, f.model ?? "", f.effort ?? null, f.modelFor ? f.machine || null : null));
}

/// The picker by the pill: harnesses, their models, their thinking.
function drawPicker(c: StartComposer) {
  const f = AG.form, el = c.pick;
  if (f.open !== "pick" || !f.agent) { closedPicker(el); return; }
  const host = formHost(), machine = f.modelFor ? f.machine || null : null;
  const name = machine ? AG.hosts?.find(m => m.machine_id === machine)?.machine_name ?? machine : null;
  const picked: Picked = { agent: f.agent, model: f.model ?? "", effort: f.effort ?? null, fast: f.fast ?? null, query: f.query ?? "" };
  drawPickerIn(el, {
    render: view => { if (c.form.isConnected) c.setPicker(view); },
    picked,
    offered: formAgents(),
    everywhere: [...new Set((AG.hosts ?? []).flatMap(m => m.agents))],
    allowed: projectHarnesses(f.project ?? null),
    machine: machine && name ? { id: machine, name: host?.machine_name ?? name } : null,
    noMachine: "Pick a machine to list its models.",
    steps: null,
    anchor: () => c.harness.isConnected ? c.harness : null,
    changed: what => {
      if (what === "models") return drawPill(c, false);
      Object.assign(f, { agent: picked.agent, model: picked.model, effort: picked.effort, fast: picked.agent === "codex" ? picked.fast : null, query: picked.query });
      // A model found under another harness keeps with it.
      if (what === "model" && f.modelFor && machine) f.modelFor = `${machine}/${picked.agent}`;
      if (what !== "agent" && !f.assistant) keepProjectPick(f.project ?? null, picked);
      if (what === "model") { f.open = null; drawParts(); return c.input.focus(); }
      drawParts();
      if (what === "agent") {
        if (f.agent && !f.assistant) keepProjectPick(f.project ?? null, { agent: f.agent, model: f.model ?? "", effort: f.effort ?? null, fast: f.fast ?? null });
        focusSearch(el);
      }
    },
  });
}

/// Machine and checkout controls, with whichever menu is open.
function drawUnder(c: StartComposer) {
  const f = AG.form, host = formHost();
  const checkout = checkoutHere();
  const shown = folderPill(f.cwd, host?.workspace, host?.home);
  const co = barCheckout(f.checkout ?? DEFAULT_CHECKOUT, baseFor(f.base, formBranches()));
  drawExtras(c);
  c.update({ choiceContents: { machine: host ? <><span className="ddot on" />{host.machine_name}</> : "Pick a machine", folder: <FolderChoice checkout={checkout} label={checkout ? co.label : shown.label} detail={checkout ? co.sub : shown.path} /> } });
  c.update({ under: <ChoiceMenuView menu={hostMenu() ?? folderMenu(checkout) ?? baseMenu() ?? accountMenu()} anchor={menuAnchor} pick={pickChoice} /> });
}

const baseButton = createRef<HTMLButtonElement>();
const accountButton = createRef<HTMLButtonElement>();

/// The named account the form starts as; null for the agent's default one.
function formAccount(): string | null {
  const f = AG.form;
  return f.assistant ? null : startAccount(accountsOf(f.agent), f.project ?? null, f.agent);
}

function drawExtras(c: StartComposer) {
  const f = AG.form, base = baseFor(f.base, formBranches()) || "HEAD";
  const folder = f.typing ? <FolderInput value={f.cwd} menu={menu} change={typeFolder} keyDown={folderKey} leave={() => { if (AG.form.typing) doneTyping(); }} pick={pickFolder} /> : null;
  const branch = checkoutHere() && makesBranch(f.checkout ?? DEFAULT_CHECKOUT)
    ? f.baseTyping
      ? <BaseInput value={f.base ?? ""} placeholder={base} change={value => { f.base = value.trim() || null; drawExtras(c); }} keyDown={baseKey} leave={() => { if (AG.form.baseTyping) { AG.form.baseTyping = false; drawParts(); } }} />
      : <BaseChoice base={base} open={f.open === "base"} button={baseButton} toggle={toggleBase} />
    : null;
  const account = !f.assistant && accountsOf(f.agent).length ? <AccountChoice account={formAccount()} open={f.open === "account"} button={accountButton} toggle={toggleAccount} /> : null;
  c.update({ extras: <>{folder}{branch}{account}</> });
}

/// The pill the open menu hangs from, and the area it stays in.
function menuAnchor() {
  const open = AG.form.open, c = startView?.composer;
  if (open === "base") return baseButton.current;
  if (open === "account") return accountButton.current;
  return c && (open === "host" || open === "folder") ? c.choice(open) : null;
}


/// The Base menu: the project's branches, the default first, and a row to
/// type any branch, tag or commit.
function baseMenu(): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "base") return null;
  const list = formBranches(), picked = baseFor(f.base, list);
  const rows: ChoiceRow[] = baseRows(list).map(name => ({ kind: "button", action: "agb", value: name, selected: picked === name, label: <span className="mono">{name}</span>, detail: name === list?.default_branch ? "default" : "" }));
  const note: ChoiceRow[] = list === undefined ? [{ kind: "button", action: null, value: "loading", disabled: true, label: <span><Spinner /> Reading branches…</span>, detail: "" }] : [];
  return { style: "base", rows: [...note, ...rows, ...(rows.length || note.length ? [{ kind: "rule" } as const] : []), { kind: "button", action: "agbtype", value: "1", label: "Type a branch or commit", detail: "" }] };
}

/// The Account menu: the agent's default account, then its named ones.
function accountMenu(): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "account") return null;
  const picked = formAccount();
  const rows: ChoiceRow[] = [null, ...accountsOf(f.agent)].map(name => ({ kind: "button", action: "aga", value: name ?? "", selected: picked === name, label: <span>{name ?? "Default"}</span>, detail: name ? "" : "as set up in Agent accounts" }));
  return { style: "", rows };
}

function toggleAccount() {
  const f = AG.form;
  if (S.view !== "agentnew" || formBusy()) return;
  f.open = f.open === "account" ? null : "account";
  drawParts();
}

/// The base field: Enter keeps what is typed; empty goes back to the default.
function baseKey(e: ReactKeyboardEvent<HTMLInputElement>) {
  if (e.key === "Enter" && !e.metaKey) { e.preventDefault(); AG.form.baseTyping = false; drawParts(); }
}

function hostMenu(): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "host") return null;
  const rows = machineRows(AG.hosts ?? [], MACHINES.list?.filter(m => m.can_host !== false).map(m => ({ id: m.id, name: m.name, online: m.online })) ?? null, a => agentKind(a).name, AGENT_ORDER);
  return { style: "cols", rows: rows.map(m => ({ kind: "button", action: "agm", value: m.id, selected: f.machine === m.id, disabled: !m.enabled, dot: m.online ? "on" : "off", label: <span>{m.name}</span>, detail: m.note })) };
}

/// Whether the folder is one of the project's checkouts on the machine
/// picked (any of them before one is).
function checkoutHere() {
  const f = AG.form, cwd = f.cwd.trim();
  if (!cwd || !f.places?.length) return false;
  const machine = f.machine || null;
  return f.places.some(p => (!machine || p.machine === machine) && (p.cwd === cwd || folderOn(p.cwd, null, null) === cwd));
}

function folderMenu(checkout: boolean): ChoiceMenu | null {
  const f = AG.form;
  if (f.open !== "folder") return null;
  const machine = f.machine || null, host = formHost();

  const places = [...new Set((f.places ?? []).filter(p => !machine || p.machine === machine).map(p => machine ? p.cwd : folderOn(p.cwd, null, null)))];
  const pick = f.checkout ?? DEFAULT_CHECKOUT;
  const same = (p: CheckoutPick) => JSON.stringify(p) === JSON.stringify(pick);
  const trees = checkout ? knownWorktrees(sessions, f.machine || null, f.cwd) : [];
  const workspace = host?.workspace ? tildePath(host.workspace, host.home) : "~";
  const rows: ChoiceRow[] = [
    ...places.map(p => ({ kind: "button", action: "agf", value: p, selected: f.cwd.trim() === p, label: <span className="mono">{tildePath(p, host?.home)}</span>, detail: "checkout" } as const)),
    ...(checkout ? [{ kind: "rule" } as const,
      { kind: "button", action: "agk", value: "current", selected: same("current"), label: "Current checkout", detail: "as it is" } as const,
      { kind: "button", action: "agk", value: "branch", selected: same("branch"), label: "New branch", detail: "in the checkout, named from the prompt" } as const,
      { kind: "button", action: "agk", value: "new", selected: same("new"), label: "New worktree", detail: "new branch in .worktrees/" } as const,
      ...trees.map(t => ({ kind: "button", action: "agw", value: t, selected: same({ worktree: t }), label: <span className="mono">{t.split("/").slice(-2).join("/")}</span>, detail: "worktree" } as const))] : []),
    ...(places.length || checkout ? [{ kind: "rule" } as const] : []),
    { kind: "button", action: "agf", value: "", selected: !f.cwd.trim(), label: "Workspace", detail: workspace, mono: true },
    { kind: "button", action: "agtype", value: "1", label: "Type a folder", detail: "" },
  ];
  return { style: "folder", rows };
}

/// Shift+Tab in the prompt moves the form to the next machine that can start it: one that runs
/// the assistant for the assistant, one with a checkout for a project.
function switchMachine(e: KeyboardEvent) {
  const f = AG.form;
  if (e.key !== "Tab" || !e.shiftKey || e.altKey || e.ctrlKey || e.metaKey || e.isComposing || formBusy()) return;
  const hosts = (AG.hosts ?? []).filter(h => !f.assistant || h.assistant);
  const next = nextBarMachine(f.places ?? null, hosts, f.machine);
  if (!next) return;

  e.preventDefault();
  pickChoice("agm", next.machine_id);
}

/// Opens or closes the machine or folder menu.
function toggleChoice(which: "host" | "folder") {
  const f = AG.form;
  if (S.view !== "agentnew" || formBusy()) return;
  f.open = f.open === which ? null : which;
  drawParts();
}

function toggleBase() {
  const f = AG.form;
  if (S.view !== "agentnew" || formBusy()) return;
  // Without the project's branches, the ref is typed.
  if (formBranches() === null) return typeBase();
  f.open = f.open === "base" ? null : "base";
  drawParts();
}

/// A row picked in the open menu.
function pickChoice(action: ChoiceAction, value: string) {
  const f = AG.form;
  if (S.view !== "agentnew" || formBusy()) return;

  switch (action) {
    case "agb": f.base = value; break;
    case "aga": if (f.agent) keepProjectAccount(f.project ?? null, f.agent, value || null); break;
    case "agbtype": return typeBase();
    case "agm":
      f.machine = value;
      f.machinePicked = true;
      if (f.assistant) keepAssistantMachine(f.machine); else keepMachine(f.project, f.machine);
      f.cwd = folderOn(f.cwd, f.machine, f.places);
      break;
    case "agf": f.cwd = value; f.checkout = DEFAULT_CHECKOUT; break;
    case "agk": if (isCheckoutKind(value)) f.checkout = value; break;
    case "agw": f.checkout = { worktree: value }; break;
    case "agtype":
      f.typing = true;
      f.open = null;
      drawParts();
      return lookFolders();
  }

  f.open = null;
  drawParts();
}

const isCheckoutKind = (value: string): value is "current" | "branch" | "new" => value === "current" || value === "branch" || value === "new";

/// Turns the Base button into a field to type a ref in.
function typeBase() {
  const f = AG.form;
  f.baseTyping = true;
  f.open = null;
  drawParts();
}

/// Opens or closes the picker; open, the search takes the keys.
export function togglePicker() {
  const f = AG.form;
  if (S.view !== "agentnew" || !f.agent) return;
  f.open = f.open === "pick" ? null : "pick";
  f.query = "";
  drawParts();
}

/// Closes an open menu or the folder field; true when there was one, so
/// Esc goes no further.
export function closeAgentNewMenus() {
  const f = AG.form;
  if (S.view !== "agentnew" || (!f.open && !f.typing && !f.baseTyping)) return false;
  f.open = null;
  f.typing = false;
  f.baseTyping = false;
  drawParts();
  startView?.composer.input.focus();
  return true;
}

// ----- Typing a folder -----

let menu: FolderMenu = NO_MENU;
let lookup = 0;
let lookupTimer: ReturnType<typeof setTimeout> | undefined;
const listed = new Map<string, string[]>();

/// The folder field: typing sets the folder, the menu offers the machine's
/// folders under what is typed, Enter or a click elsewhere keeps it.
function typeFolder(value: string) {
  const f = AG.form;
  f.cwd = value;
  f.checkout = DEFAULT_CHECKOUT;
  if (startView) drawExtras(startView.composer);
  lookFolders();
}

function folderKey(e: ReactKeyboardEvent<HTMLInputElement>) {
  const act = menuKey(menu, e.key, e.shiftKey || e.altKey || e.ctrlKey || e.metaKey);

  if (act.type === "none") {
    if (e.key === "Enter" && !e.metaKey) { e.preventDefault(); doneTyping(); }
    return;
  }

  e.preventDefault();
  e.stopPropagation();

  if (act.type === "move") showMenu({ ...menu, index: act.index });
  else if (act.type === "close") showMenu(NO_MENU);
  else typeFolder(act.text);
}

function doneTyping() {
  const f = AG.form;
  f.typing = false;
  menu = NO_MENU;
  lookup++;
  clearTimeout(lookupTimer);
  drawParts();
}

/// Offers the folders under what is typed, on the machine picked; nothing
/// before one is.
function lookFolders() {
  clearTimeout(lookupTimer);
  const ask = ++lookup, machine = AG.form.machine || null, typed = splitTyped(AG.form.cwd);
  if (!machine || !typed) return showMenu(NO_MENU);
  const key = `${machine}\n${typed.dir}`;
  const offer = (folders: readonly string[]) => { if (ask === lookup) showMenu(menuOf(typed.dir, suggest(folders, typed.prefix))); };
  const known = listed.get(key);
  if (known) return offer(known);
  lookupTimer = setTimeout(async () => {
    try {
      const listing = await invoke<MachineFolder>("list_machine_folder", { machineId: machine, path: typed.dir });
      listed.set(key, listing.folders);
      offer(listing.folders);
    } catch {
      if (ask === lookup) showMenu(NO_MENU);
    }
  }, 150);
}

function showMenu(next: FolderMenu) {
  menu = AG.form.typing ? next : NO_MENU;
  if (startView && S.view === "agentnew") drawExtras(startView.composer);
}

function pickFolder(index: number) {
  if (!AG.form.typing || !isOpen(menu)) return;
  typeFolder(completed(menu.dir, menu.items[index]));
}

// ----- Models -----

function restoreProjectPick() {
  const f = AG.form, saved = lastProjectPick(f.project ?? null);
  f.agent = saved?.agent ?? null;
  f.model = saved?.model ?? "";
  f.effort = saved?.effort ?? null;
  f.fast = saved?.fast ?? null;
  f.modelFor = "";
}

/// The assistant setting, once read for the form open now; null until then.
let assistantPick: AssistantPick | null = null;

/// Presets the form to the assistant setting once it is read.
function presetAssistant() {
  const f = AG.form;
  assistantPick = null;
  f.model = "";
  f.modelFor = "";
  loadAssistant().then(pick => {
    if (AG.form !== f) return;
    assistantPick = pick;
    // The machine is picked again for the harness.
    f.machine = "";
    f.machinePicked = false;
    f.modelFor = "";
    if (S.view === "agentnew") drawParts();
  }, e => console.warn("assistant setting:", e));
}

/// The harness, model and thinking level the form starts on: the assistant
/// setting, or the ones last picked in the project.
function formPick(): { agent: string | null; model: string; effort: string | null; fast: boolean | null } | null {
  const f = AG.form;
  if (!f.assistant) return lastProjectPick(f.project ?? null);
  if (!assistantPick || !AG.hosts) return null;
  const pick = resolveAssistant(assistantPick, AG.hosts);
  return { ...pick, model: pick.model ?? "" };
}

/// Keeps the model with the machine and harness it was picked for, and
/// asks for their models when not known lately.
function syncModel() {
  const f = AG.form, machine = f.machine || null, agent = f.agent;
  const key = machine && agent ? `${machine}/${agent}` : "";
  if (f.modelFor !== key) {
    const saved = !f.modelFor ? formPick() : null;
    f.model = saved?.agent === agent ? saved.model : "";
    if (saved?.agent === agent) { f.effort = keepEffort(agent, saved.effort); f.fast = saved.fast ?? null; }
    f.modelFor = key;
  }
  if (!machine || !agent) return;
  modelsOn(machine, agent, () => { if (S.view === "agentnew" && AG.form.modelFor === key && startView) drawPill(startView.composer, false); });
}

// ----- Start -----

export async function startAgent() {
  const view = startView, f = AG.form;
  if (S.view !== "agentnew" || !view || view.phase.kind !== "editing" || view.attachments.reading || view.composer.input.readOnly) return;
  const { attachments, composer } = view;

  if (!f.prompt.trim() && !attachments.count) {
    view.phase = { kind: "editing", error: { kind: "prompt" } };
    drawParts();
    return composer.input.focus();
  }
  if (!f.machine) { f.open = "host"; return drawParts(); }

  const { agent, assistant, machine, machinePicked } = f;
  const back = AG.back, hosts = AG.hosts ?? [], typed = f.prompt, folder = f.cwd.trim();
  const model = f.model?.trim() && f.modelFor ? modelId(knownModels(machine, agent), f.model) : null;
  const effort = f.effort || null, fast = f.fast ?? null;
  const account = projectAccount(f.project ?? null, agent);
  const images = attachments.images.map(sentImage);
  const hasFiles = attachments.files.pending.length > 0;
  const root = folder || formHost()?.workspace || "";
  // A new branch or worktree is made only when it was selected.
  const { cwd, checkout } = checkoutHere()
    ? startPlace(f.checkout ?? DEFAULT_CHECKOUT, folder, baseFor(f.base, formBranches()))
    : { cwd: folder, checkout: null };
  const showing = () => startView === view && S.view === "agentnew";

  view.phase = { kind: "preparing" };
  f.open = null;
  drawParts();

  try {
    if (!assistant) keepMachine(f.project, machine);
    const prompt = await attachments.files.prompt(composeMessage(typed), machine, root);
    const accounts = await loadAccountNames();

    view.phase = { kind: "starting" };
    if (showing() && hasFiles) composer.update({ progress: "Starting the agent…" });
    const started = assistant
      ? await startAssistant(prompt, { agent, model, effort, fast }, hosts, machinePicked || hasFiles ? machine : null, images)
      : await start({ machine, agent, cwd: cwd || null, prompt, images, model, effort, fast: agent === "codex" ? fast : null, checkout, assistant: false,
        account: accounts.some(named => named.agent === agent && named.name === account) ? account : null });

    attachments.clear();
    view.phase = { kind: "editing", error: null };
    if (assistant && assistantDraft === typed && (startView === view || !startView || !AG.form.assistant)) assistantDraft = "";
    if (showing()) openSession(started.session, { why: started.why, back });
  } catch (e) {
    const message = String(e).replace(/^bad request: /, "");
    view.phase = { kind: "editing", error: { kind: "failed", message } };
    if (startView !== view) {
      attachments.discard();
    } else if (showing()) {
      composer.update({ progress: "" });
      drawParts();
    }
  }
}

/// Starts the Home assistant in a machine's home folder with `pick`, the
/// setting as `loadAssistant` reads it, resolved against `hosts`: on `machine`
/// when the user picked one, else on the one it last ran on while that offers
/// the harness, else on the one Auto picks, this computer first. Remembers
/// where it ran.
export async function startAssistant(prompt: string, pick: AssistantPick, hosts: readonly AgentHost[], machine: string | null, images: readonly SentImage[] = []): Promise<Started> {
  const resolved = resolveAssistant(pick, hosts);
  const last = hosts.find(host => host.machine_id === lastAssistantMachine());
  const lastOffers = !!resolved.agent && !!last?.assistant && last.agents.includes(resolved.agent);
  const started = await start({ machine: machine ?? (lastOffers ? last.machine_id : null), agent: resolved.agent, cwd: null, prompt, images, model: resolved.model, effort: resolved.effort, fast: resolved.fast ?? null, checkout: null, assistant: true, account: null });
  keepAssistantMachine(started.session.machine_id);

  return started;
}

/// What `agent_begin` takes.
interface Start {
  machine: string | null;
  agent: string | null;
  cwd: string | null;
  prompt: string;
  images: readonly SentImage[];
  model: string | null;
  effort: string | null;
  fast: boolean | null;
  checkout: CheckoutPlan | null;
  assistant: boolean;
  /// The agent's named account; null for its default one.
  account: string | null;
}

/// Hands a start to this device, which answers at once with the session it shows as until its
/// machine names it, and keeps the model and thinking level it starts on for its pill, and its
/// first message for its log.
async function start(s: Start): Promise<Started> {
  const prompt = withMentions(s.prompt, [], s.images.length);
  const started = await invoke<Started>("agent_begin", { machineId: s.machine, agent: s.agent, cwd: s.cwd, prompt, images: s.images, model: s.model, effort: s.effort, fast: s.fast ?? null, checkout: s.checkout, assistant: s.assistant, account: s.account });
  const resolved = s.model ?? knownModels(s.machine, s.agent)?.current ?? "";
  sessionPicks.set(started.session.id, { model: resolved.toLowerCase() === "default" ? "" : resolved, effort: s.effort, fast: s.fast ?? null });
  startedWith(started.session, prompt, s.images);

  // The start's row may leave out that the session is the assistant, as a released node's does.
  return s.assistant ? { ...started, session: { ...started.session, assistant: true } } : started;
}
