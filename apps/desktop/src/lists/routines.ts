import { $, $$, bar, S } from "../bar/core.ts";
import { appTile, fillAppTiles } from "../chat/applogo.ts";
import { hints } from "../bar/hints.ts";
import { openSession } from "../chat/history.ts";
import { arrowKey, clampSel, CLOCK_ICON, confirmKey, confirmRow, dayDiff, dayMonth, emptyPage, errLine, type ListState, loadList, PLAY_ICON, PLUS_ICON } from "./memories.ts";
import { switchHtml, esc } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";
import { confirmSheet, openMenu } from "../ui/menu.ts";

// ---------- Routines ----------

/// A routine as `list_crons` sends it.
export interface Cron {
  id: string;
  label: string;
  prompt: string;
  /// RFC 3339 for a one-off, `arrive:` or `leave:` for a place, else cron.
  schedule: string;
  enabled: boolean;
  created_at: string;
  run_once: boolean;
  last_run_at: string | null;
  next_run_at: string | null;
}

type RoutineMode = "list" | "actions" | "delete";
type RoutineAction = RoutineMode | "new" | "grants" | "edit" | "toggle" | "delete-yes" | "run";

export const R: ListState<Cron> & { mode: RoutineMode } = { list: [], sel: 0, mode: "list", error: "", loaded: false };

/// How the form repeats a routine; place is a routine a place starts, which
/// the form leaves as it is.
type Repeat = "day" | "weekdays" | "week" | "hour" | "once" | "cron" | "place";

/// The routine form, new or editing `edit`.
interface RoutineForm {
  edit: Cron | null;
  label: string;
  prompt: string;
  repeat: Repeat;
  /// Cron day numbers, Sunday 0.
  days: number[];
  /// `HH:MM`, and `YYYY-MM-DD` for a one-off.
  time: string;
  date: string;
  cron: string;
  /// "prompt" when the prompt is missing, else what to show.
  error: string;
  busy: boolean;
}
export const NR: RoutineForm = { edit: null, label: "", prompt: "", repeat: "day", days: [1], time: "08:00", date: "", cron: "", error: "", busy: false };
export const CRON_DAYS = [1, 2, 3, 4, 5, 6, 0];
export const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
export const DAY_FULL = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

export const clock = (h: number, m: number) => `${h}:${String(m).padStart(2, "0")}`;
export const pad2 = (n: number | string) => String(n).padStart(2, "0");
export const dateValue = (d: Date) => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;

/// "today 9:00", "tomorrow 9:00", "Mon 9:00" or, further out, "12 Oct 9:00".
export function describeDay(at: Date) {
  const ahead = dayDiff(new Date(), at);
  const label = ahead === 0 ? "today" : ahead === 1 ? "tomorrow" : ahead > 1 && ahead < 7 ? DAY_NAMES[at.getDay()] : dayMonth(at);
  return `${label} ${clock(at.getHours(), at.getMinutes())}`;
}

/// The five fields of `s` when it is a cron at a set minute on any day of
/// any month, the only shapes the form writes; else null.
function cronParts(s: string) {
  const p = s.trim().split(/\s+/);
  return p.length !== 5 || p[2] !== "*" || p[3] !== "*" || !/^\d+$/.test(p[0]) ? null : p;
}

const PLACE_LIMIT: Record<string, string> = { daily: " · once a day at most", once: " · next time only" };

/// Reads the cron shapes the form writes as words, the same as the phone. Anything else shows as written.
export function routineWhen(s: string) {
  const place = /^(arrive|leave):[^:]+(?::(daily|once))?$/.exec(s);
  if (place) return (place[1] === "arrive" ? "When you arrive at a saved place" : "When you leave a saved place")
    + (PLACE_LIMIT[place[2]] ?? "");
  if (/^\d{4}-\d\d-\d\dT/.test(s)) {
    const at = new Date(s);
    if (!isNaN(at.getTime())) return "Once, " + describeDay(at);
  }
  const p = cronParts(s);
  if (!p) return s;
  const min = Number(p[0]), days = p[4];
  if (p[1] === "*" && days === "*") return min ? `Every hour at :${pad2(min)}` : "Every hour";
  if (!/^\d+$/.test(p[1])) return s;
  const at = clock(Number(p[1]), min);
  if (days === "*") return `Every day at ${at}`;
  if (days === "1-5") return `Weekdays at ${at}`;
  if (/^[0-6]$/.test(days)) return `Every ${DAY_FULL[Number(days)]} at ${at}`;
  if (/^[0-6](,[0-6])*$/.test(days)) return days.split(",").map(d => DAY_NAMES[Number(d)]).join(", ") + ` at ${at}`;
  return s;
}

export function renderRoutines() {
  S.view = "routines";
  invoke("close_group").catch(() => {});
  Object.assign(R, { mode: "list", error: "" });
  drawRoutines();
  loadRoutines();
}

export async function loadRoutines() {
  // The header's Grants count; a failure leaves it out.
  invoke<Grant[]>("list_grants").then(list => { GR.list = list; GR.loaded = true; if (S.view === "routines") drawRoutines(); }).catch(() => {});
  await loadList(R, "list_crons");
  if (S.view === "routines") drawRoutines();
}

function routineRow(c: Cron, i: number) {
  const cur = i === R.sel, label = esc(c.label || "Reminder");
  if (cur && R.mode === "delete") return confirmRow(i, `Delete <b>${label}</b>?`, "data-r", "Delete");
  const next = c.enabled ? (c.next_run_at && !c.run_once ? "Next: " + describeDay(new Date(c.next_run_at)) : "") : "Paused";
  const tail = cur && R.mode === "actions"
    ? `<span class="acts"><button class="dbtn" data-r="run">${PLAY_ICON}Run now</button><button class="dbtn" data-r="edit">Edit<kbd>⌘E</kbd></button><button class="dbtn warn" data-r="delete">Delete<kbd>⌘⌫</kbd></button></span>`
    : switchHtml(c.enabled, `${label} on`, `data-r="toggle" data-i="${i}"`);
  return `<div class="drt${c.enabled ? "" : " off"}" role="option" aria-selected="${cur}" data-i="${i}"><span class="ic">${CLOCK_ICON}</span>
        <span class="tt"><b>${label}</b><span>${esc(c.prompt)}</span><small>${esc([routineWhen(c.schedule), next].filter(Boolean).join(" · "))}</small></span>${tail}</div>`;
}

export function drawRoutines() {
  clampSel(R);
  const on = R.list.filter(c => c.enabled).length, off = R.list.length - on;
  const sum = !R.loaded ? "" : !R.list.length ? "Nothing scheduled" : [on && `${on} on`, off && `${off} paused`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Routines</b><small>${sum}</small></span><button class="dbtn" data-r="grants">Grants${GR.list.length ? ` <span class="dmut">${GR.list.length}</span>` : ""}<kbd>⌘G</kbd></button><button class="dbtn" data-r="new">${PLUS_ICON}New<kbd>⌘N</kbd></button></div>`;
  if (R.loaded && !R.list.length) {
    bar.innerHTML = head + errLine(R.error) + emptyPage(CLOCK_ICON, "No routines yet.", "Schedule tasks for Beep to run on a timer.", '<button class="dbtn pri" data-r="new">New routine</button>')
      + hints([["⌘N", "new routine"], ["esc", "back"]]);
  } else {
    bar.innerHTML = head + errLine(R.error) + `<div class="dlist top" id="rlist" role="listbox" aria-label="Routines">${R.list.map(routineRow).join("")}</div>`
      + hints(R.mode === "actions" ? [["⌘E", "edit"], ["tab", "hide actions"], ["esc", "back"]] : [["↑↓", "move"], ["space", "on or off"], ["tab", "actions"], ["⌘N", "new routine"], ["⌘G", "grants"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = routineClick;
  bar.oncontextmenu = routineMenu;
}

/// A routine row's right-click menu: what its buttons do. Delete asks first.
function routineMenu(e: MouseEvent) {
  if (S.view !== "routines") return;
  const row = (e.target as HTMLElement).closest<HTMLElement>(".drt[data-i]");
  if (!row) return;
  R.sel = Number(row.dataset.i);
  const c = R.list[R.sel];
  openMenu(e, c.label, [
    { label: "Run now", on: () => void routineAction("run") },
    { label: "Edit", on: () => void routineAction("edit") },
    { label: c.enabled ? "Pause" : "Resume", on: () => void routineAction("toggle") },
    {
      label: "Delete…",
      warn: true,
      on: () => confirmSheet({
        title: `Delete ${c.label}?`,
        text: "The routine stops running.",
        action: "Delete",
        run: () => routineAction("delete-yes"),
      }),
    },
  ]);
}

export function routineClick(e: MouseEvent) {
  if (S.view !== "routines") return;
  const target = e.target as HTMLElement;
  const btn = target.closest<HTMLElement>("[data-r]");
  const row = target.closest<HTMLElement>(".drt[data-i]");
  if (btn?.dataset.i) R.sel = Number(btn.dataset.i);
  if (btn) return routineAction(btn.dataset.r as RoutineAction);
  if (row) {
    R.sel = Number(row.dataset.i);
    R.mode = "actions";
    drawRoutines();
  }
}

export async function routineAction(a: RoutineAction) {
  const c = R.list[R.sel];
  if (a === "new") return renderRoutineNew();
  if (a === "grants") return renderGrants();
  if (a === "edit") return c && renderRoutineEdit(c);
  if (a === "list" || a === "actions" || a === "delete") {
    if (!c && a !== "list") return;
    R.mode = a;
    return drawRoutines();
  }
  if (!c) return;
  try {
    if (a === "toggle") {
      c.enabled = await invoke<boolean>("toggle_cron", { id: c.id });
    } else if (a === "delete-yes") {
      await invoke("delete_cron", { id: c.id });
      R.list = R.list.filter(x => x.id !== c.id);
    } else if (a === "run") {
      const session = await invoke<string>("run_cron", { id: c.id });
      return openSession(session);
    }
    R.error = "";
  } catch (e) {
    R.error = String(e);
  }
  R.mode = "list";
  if (S.view === "routines") drawRoutines();
}

export function routineKeys(e: KeyboardEvent, cmd: boolean) {
  if (R.mode === "delete") return confirmKey(e, () => routineAction("delete-yes"));
  if (arrowKey(e, R)) { R.mode = "list"; return drawRoutines(); }
  if (e.key === "Tab" && R.list.length) { e.preventDefault(); return routineAction(R.mode === "actions" ? "list" : "actions"); }
  if (e.key === " " && R.mode === "list" && R.list.length) { e.preventDefault(); return routineAction("toggle"); }
  if (cmd && e.key === "Backspace" && R.list.length) { e.preventDefault(); return routineAction("delete"); }
  if (e.key === "Enter" && R.mode === "actions") { e.preventDefault(); return routineAction("run"); }
}

/// The schedule the form describes, or "" when it is not complete.
export function routineSchedule() {
  if (NR.repeat === "cron") return NR.cron.trim();
  const [h, m] = NR.time.split(":").map(Number);
  if (isNaN(h) || isNaN(m)) return "";
  if (NR.repeat === "hour") return `${m} * * * *`;
  if (NR.repeat === "day") return `${m} ${h} * * *`;
  if (NR.repeat === "weekdays") return `${m} ${h} * * 1-5`;
  if (NR.repeat === "week") {
    const days = CRON_DAYS.filter(d => NR.days.includes(d));
    return days.length ? `${m} ${h} * * ${days.join(",")}` : "";
  }
  if (!NR.date) return "";
  const at = new Date(`${NR.date}T${NR.time}`);
  return isNaN(at.getTime()) ? "" : at.toISOString();
}

/// Opens the form on `edit`, or empty for a new routine.
export function renderRoutineNew(edit: Cron | null = null, fields: Partial<RoutineForm> = {}) {
  S.view = "rnew";
  Object.assign(NR, { edit, label: "", prompt: "", repeat: "day", days: [1], time: "08:00", cron: "", error: "", busy: false, date: dateValue(new Date()) }, fields);
  drawRoutineNew();
  $$("#nrlabel").focus();
}

/// The form fields for `schedule`, the shapes routineSchedule writes read
/// back. A place trigger stays as it is, anything else shows as cron.
export function routineFields(schedule: string): Partial<RoutineForm> {
  if (/^(arrive|leave):/.test(schedule)) return { repeat: "place" };
  if (/^\d{4}-\d\d-\d\dT/.test(schedule)) {
    const at = new Date(schedule);
    if (!isNaN(at.getTime())) return { repeat: "once", date: dateValue(at), time: `${pad2(at.getHours())}:${pad2(at.getMinutes())}` };
  }
  const p = cronParts(schedule);
  const cron: Partial<RoutineForm> = { repeat: "cron", cron: schedule };
  if (!p) return cron;
  if (p[1] === "*") return p[4] === "*" ? { repeat: "hour", time: `00:${pad2(p[0])}` } : cron;
  if (!/^\d+$/.test(p[1])) return cron;
  const time = `${pad2(p[1])}:${pad2(p[0])}`;
  if (p[4] === "*") return { repeat: "day", time };
  if (p[4] === "1-5") return { repeat: "weekdays", time };
  if (/^[0-6](,[0-6])*$/.test(p[4])) return { repeat: "week", time, days: p[4].split(",").map(Number) };
  return cron;
}

/// Edits routine `c`'s name and schedule. What it does stays as it is.
export function renderRoutineEdit(c: Cron) {
  renderRoutineNew(c, { label: c.label, prompt: c.prompt, ...routineFields(c.schedule) });
}

export function drawRoutineNew() {
  const chip = (k: Repeat, l: string) => `<button class="dchip" aria-pressed="${NR.repeat === k}" data-rep="${k}">${l}</button>`;
  const days = CRON_DAYS.map(d => `<button class="dchip day" aria-pressed="${NR.days.includes(d)}" data-day="${d}">${DAY_NAMES[d]}</button>`).join("");
  const time = `<label class="dfield dnarrow"><span>${NR.repeat === "hour" ? "Minute past" : "Time"}</span><input id="nrtime" type="time" value="${esc(NR.time)}"></label>`;
  const when = NR.repeat === "cron"
    ? `<label class="dfield"><span>Cron expression</span><input id="nrcron" class="mono plain" placeholder="30 7 * * 1-5" value="${esc(NR.cron)}" spellcheck="false"></label>
       <span class="dmuted dpull" id="nrcronsays"></span>`
    : NR.repeat === "once"
      ? `<div class="dtwo"><label class="dfield"><span>Date</span><input id="nrdate" type="date" value="${esc(NR.date)}"></label>${time.replace(" dnarrow", "")}</div>`
      : (NR.repeat === "week" ? `<div class="dchips">${days}</div>` : "") + time;
  const missing = NR.error === "prompt", edit = NR.edit;
  const text = (t: string) => `<div class="dtext">${esc(t)}</div>`;
  const task = edit
    ? `<div class="dfield"><span>What Beep does</span>${text(NR.prompt)}</div>`
    : `<label class="dfield${missing ? " err" : ""}"><span>${missing ? "Tell Beep what to do" : "What should Beep do?"}</span>
        <textarea id="nrprompt" rows="2" placeholder="Summarise my calendar and the weather">${esc(NR.prompt)}</textarea></label>`;
  const schedule = NR.repeat === "place"
    ? `<div class="dfield"><span>Starts</span>${text(routineWhen(edit!.schedule))}<span>Change the place in a chat.</span></div>`
    : `<div class="dfield"><span>Repeat</span><div class="dchips">${chip("day", "Every day")}${chip("weekdays", "Weekdays")}${chip("week", "Weekly")}${chip("hour", "Every hour")}${chip("once", "Once")}${chip("cron", "Cron")}</div></div>
      ${when}`;
  const go = edit ? "Save" : "Create";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>${edit ? "Edit routine" : "New routine"}</b><small>${edit ? esc(edit.label || "Reminder") : "Runs on this account, phone or computer off"}</small></span></div>
    <div class="dform dscroll">
      <label class="dfield"><span>Name</span><input id="nrlabel" placeholder="Morning briefing" value="${esc(NR.label)}"></label>
      ${task}
      ${schedule}
      ${NR.error && !missing ? `<div class="derr">${esc(NR.error)}</div>` : ""}
      <div class="dline"><button class="dbtn pri" id="nrgo"${NR.busy ? " disabled" : ""}>${go}<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn quiet" id="nrno">Cancel</button>
        <span class="dmuted dpush">${edit ? "To change what it does, ask Beep in a chat" : "Or just tell Beep in a chat"}</span></div>
    </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, go.toLowerCase()], ["esc", "cancel"]])}`;
  const bind = (id: string, key: "label" | "prompt" | "time" | "date" | "cron") =>
    $(id)?.addEventListener("input", e => { NR[key] = (e.target as HTMLInputElement).value; if (id === "#nrcron") cronSays(); });
  bind("#nrlabel", "label"); bind("#nrprompt", "prompt"); bind("#nrtime", "time"); bind("#nrdate", "date"); bind("#nrcron", "cron");
  cronSays();
  bar.onclick = e => {
    if (S.view !== "rnew") return;
    const target = e.target as HTMLElement;
    const rep = target.closest<HTMLElement>("[data-rep]"), day = target.closest<HTMLElement>("[data-day]");
    if (rep) { NR.repeat = rep.dataset.rep as Repeat; drawRoutineNew(); }
    else if (day) {
      const d = Number(day.dataset.day);
      NR.days = NR.days.includes(d) ? NR.days.filter(x => x !== d) : [...NR.days, d];
      day.setAttribute("aria-pressed", String(NR.days.includes(d)));
    } else if (target.closest("#nrgo")) createRoutine();
    else if (target.closest("#nrno")) renderRoutines();
  };
}

export function cronSays() {
  const el = $("#nrcronsays");
  if (!el) return;
  const said = NR.cron.trim() && routineWhen(NR.cron.trim());
  el.textContent = said && said !== NR.cron.trim() ? said : "";
}

/// Why the form's schedule is not complete.
const scheduleMissing = () => NR.repeat === "week" ? "Pick at least one day" : "Set when it runs";

/// Sends `call` for the form, then shows Routines, or the form again with
/// the error.
async function submitForm(call: Promise<unknown>) {
  try {
    await call;
    if (S.view === "rnew") renderRoutines();
  } catch (e) {
    NR.error = String(e);
    NR.busy = false;
    if (S.view === "rnew") drawRoutineNew();
  }
}

export async function createRoutine() {
  if (NR.busy) return;
  if (NR.edit) return saveRoutine();
  const prompt = NR.prompt.trim(), schedule = routineSchedule();
  NR.error = !prompt ? "prompt" : !schedule ? scheduleMissing() : "";
  if (NR.error) return drawRoutineNew();
  NR.busy = true;
  return submitForm(invoke("create_cron", { label: NR.label.trim() || prompt.slice(0, 40), prompt, schedule }));
}

/// Whether two schedules are the same, a one-off in the same minute
/// written another way included.
export function sameSchedule(a: string, b: string) {
  if (a === b) return true;
  const iso = /^\d{4}-\d\d-\d\dT/;
  // The form keeps minutes only.
  const minute = (s: string) => Math.floor(new Date(s).getTime() / 60000);
  return iso.test(a) && iso.test(b) && minute(a) === minute(b);
}

/// Saves the name and schedule of the routine being edited, sending only
/// what changed.
export async function saveRoutine() {
  const c = NR.edit!;
  const schedule = NR.repeat === "place" ? c.schedule : routineSchedule();
  NR.error = !schedule ? scheduleMissing() : "";
  if (NR.error) return drawRoutineNew();
  const label = NR.label.trim();
  const changed = { label: label && label !== c.label ? label : null, schedule: sameSchedule(schedule, c.schedule) ? null : schedule };
  if (changed.label == null && changed.schedule == null) return renderRoutines();
  NR.busy = true;
  return submitForm(invoke("update_cron", { id: c.id, ...changed }));
}

// ---------- Routine grants ----------

/// What a routine may do without asking (ListGrants). Times are in
/// milliseconds since the epoch.
export interface Grant {
  id: string;
  routine_id: string;
  /// The routine's label, or its prompt when it has none.
  routine: string;
  app: string;
  action: string;
  fields: { label: string; value: string }[];
  per_run: number;
  expires_at: number;
  created_at: number;
  last_used_at: number | null;
}

type GrantMode = "list" | "confirm";
export const GR: ListState<Grant> & { mode: GrantMode } = { list: [], sel: 0, mode: "list", error: "", loaded: false };
export const SHIELD_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z"/></svg>';

/// "40 min", "5 h" or "6 d".
export function span(ms: number) {
  const min = Math.max(1, Math.round(Math.abs(ms) / 60000));
  return min < 60 ? `${min} min` : min < 48 * 60 ? `${Math.round(min / 60)} h` : `${Math.round(min / 1440)} d`;
}

export function renderGrants() {
  S.view = "grants";
  invoke("close_group").catch(() => {});
  Object.assign(GR, { sel: 0, mode: "list", error: "" });
  drawGrants();
  loadGrants();
}

export async function loadGrants() {
  await loadList(GR, "list_grants");
  if (S.view === "grants") drawGrants();
  else if (S.view === "routines") drawRoutines();
}

function grantRow(g: Grant, i: number, now: number) {
  const on = i === GR.sel, short = g.action.replace(/ for a routine$/, "");
  if (on && GR.mode === "confirm") return confirmRow(i, `Revoke? <b>${esc(g.routine)}</b> will ask before each ${esc(short)} again.`, "data-g", "Revoke");
  const per = g.per_run === 1 ? "1 each run" : `up to ${g.per_run} each run`;
  const ends = g.expires_at > now ? `Ends in ${span(g.expires_at - now)}` : "Ended";
  const used = g.last_used_at ? `Last used ${span(now - g.last_used_at)} ago` : "Not used yet";
  const acts = on ? `<span class="acts"><button class="dbtn warn" data-g="revoke">Revoke<kbd>⌘⌫</kbd></button></span>` : "";
  const kv = on && g.fields.length ? `<span class="dgkv">${g.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</span>` : "";
  return `<div class="drt dalignt" role="option" aria-selected="${on}" data-i="${i}">${appTile(g.app).replace('class="dapp"', 'class="dapp" style="margin-top:2px"')}
        <span class="tt"><b>${esc(g.routine)}</b><span>${esc(`${g.app}: ${short}`)}, ${per}</span><small>${ends} · ${used}</small>${kv}</span>${acts}</div>`;
}

export function drawGrants() {
  clampSel(GR);
  const head = `<div class="dhead"><span class="tt"><b>Routine grants</b><small>${GR.loaded && !GR.list.length ? "Nothing granted" : "What routines may do without asking"}</small></span></div>`;
  if (GR.loaded && !GR.list.length) {
    bar.innerHTML = head + errLine(GR.error) + emptyPage(SHIELD_ICON, "No routine grants.", "Beep asks in a chat when a routine needs one.") + hints([["esc", "back"]]);
  } else {
    const now = Date.now();
    bar.innerHTML = head + errLine(GR.error) + `<div class="dlist top" id="glist" role="listbox" aria-label="Routine grants">${GR.list.map((g, i) => grantRow(g, i, now)).join("")}</div>`
      + hints(GR.mode === "confirm" ? [[ICON.ret, "revoke"], ["esc", "cancel"]] : [["↑↓", "move"], ["⌘⌫", "revoke"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
    fillAppTiles(bar);
  }
  bar.onclick = grantClick;
}

export function grantClick(e: MouseEvent) {
  if (S.view !== "grants") return;
  const target = e.target as HTMLElement;
  const btn = target.closest<HTMLElement>("[data-g]"), row = target.closest<HTMLElement>(".drt[data-i]");
  // While a confirm row shows, it stays the grant Enter revokes.
  if (row && GR.mode === "list") GR.sel = Number(row.dataset.i);
  if (btn) return grantAction(btn.dataset.g!);
  if (row && GR.mode === "list") drawGrants();
}

export async function grantAction(a: string) {
  const g = GR.list[GR.sel];
  if (!g) return;
  if (a === "list" || a === "revoke") {
    GR.mode = a === "revoke" ? "confirm" : "list";
    return drawGrants();
  }
  if (a !== "revoke-yes") return;
  try {
    await invoke("revoke_grant", { id: g.id });
    GR.list = GR.list.filter(x => x.id !== g.id);
    GR.error = "";
  } catch (e) {
    GR.error = String(e);
  }
  GR.mode = "list";
  if (S.view === "grants") drawGrants();
}

export function grantKeys(e: KeyboardEvent, cmd: boolean) {
  if (GR.mode === "confirm") return confirmKey(e, () => grantAction("revoke-yes"));
  if (arrowKey(e, GR)) return drawGrants();
  if (cmd && e.key === "Backspace" && GR.list.length) { e.preventDefault(); return grantAction("revoke"); }
}
