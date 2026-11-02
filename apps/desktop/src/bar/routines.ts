import { $, bar, esc, ICON, invoke, S } from "./core.ts";
import { hints } from "./hints.ts";
import { openSession } from "./history.ts";
import { CLOCK_ICON, MONTHS, PLAY_ICON, PLUS_ICON } from "./memories.ts";

// ---------- Routines ----------

// mode: list | actions | delete
export const R = { list: [], sel: 0, mode: "list", error: "", loaded: false };
// The routine form, new or editing `edit` (a cron). repeat: day | weekdays |
// week | hour | once | cron, or place for a routine a place starts, which
// the form leaves as it is.
export const NR = { edit: null, label: "", prompt: "", repeat: "day", days: [1], time: "08:00", date: "", cron: "", error: "", busy: false };
export const CRON_DAYS = [1, 2, 3, 4, 5, 6, 0];
export const DAY_NAMES = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
export const DAY_FULL = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

export const clock = (h, m) => `${h}:${String(m).padStart(2, "0")}`;

/// "today 9:00", "tomorrow 9:00", "Mon 9:00" or, further out, "12 Oct 9:00".
export function describeDay(at) {
  const day = d => new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const ahead = Math.round((day(at) - day(new Date())) / 86400000);
  const label = ahead === 0 ? "today" : ahead === 1 ? "tomorrow" : ahead > 1 && ahead < 7 ? DAY_NAMES[at.getDay()] : `${at.getDate()} ${MONTHS[at.getMonth()]}`;
  return `${label} ${clock(at.getHours(), at.getMinutes())}`;
}

/// Reads the cron shapes the form writes as words, the same as the phone. Anything else shows as written.
export function routineWhen(s) {
  const place = /^(arrive|leave):[^:]+(?::(daily|once))?$/.exec(s);
  if (place) return (place[1] === "arrive" ? "When you arrive at a saved place" : "When you leave a saved place")
    + ({ daily: " · once a day at most", once: " · next time only" }[place[2]] ?? "");
  if (/^\d{4}-\d\d-\d\dT/.test(s)) {
    const at = new Date(s);
    if (!isNaN(at)) return "Once, " + describeDay(at);
  }
  const p = s.trim().split(/\s+/);
  if (p.length !== 5 || p[2] !== "*" || p[3] !== "*" || !/^\d+$/.test(p[0])) return s;
  const min = Number(p[0]), days = p[4];
  if (p[1] === "*" && days === "*") return min ? `Every hour at :${String(min).padStart(2, "0")}` : "Every hour";
  if (!/^\d+$/.test(p[1])) return s;
  const at = clock(Number(p[1]), min);
  if (days === "*") return `Every day at ${at}`;
  if (days === "1-5") return `Weekdays at ${at}`;
  if (/^[0-6]$/.test(days)) return `Every ${DAY_FULL[days]} at ${at}`;
  if (/^[0-6](,[0-6])*$/.test(days)) return days.split(",").map(d => DAY_NAMES[d]).join(", ") + ` at ${at}`;
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
  invoke("list_grants").then(list => { GR.list = list; GR.loaded = true; if (S.view === "routines") drawRoutines(); }).catch(() => {});
  try {
    R.list = await invoke("list_crons");
    R.loaded = true;
    R.error = "";
  } catch (e) {
    R.error = String(e);
  }
  if (S.view === "routines") drawRoutines();
}

export function drawRoutines() {
  R.sel = Math.max(0, Math.min(R.sel, R.list.length - 1));
  const on = R.list.filter(c => c.enabled).length, off = R.list.length - on;
  const sum = !R.loaded ? "" : !R.list.length ? "Nothing scheduled" : [on && `${on} on`, off && `${off} paused`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Routines</b><small>${sum}</small></span><button class="dbtn" data-r="grants">Grants${GR.list.length ? ` <span style="color:var(--muted)">${GR.list.length}</span>` : ""}<kbd>⌘G</kbd></button><button class="dbtn" data-r="new">${PLUS_ICON}New<kbd>⌘N</kbd></button></div>`;
  const err = R.error ? `<div class="derr" style="padding:8px 18px 0">${esc(R.error)}</div>` : "";
  if (R.loaded && !R.list.length) {
    bar.innerHTML = head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${CLOCK_ICON}</span>
      <b style="font-size:14px">No routines yet.</b><span class="dmuted" style="display:block">Schedule tasks for Beep to run on a timer.</span><button class="dbtn pri" data-r="new" style="margin-top:4px">New routine</button></div>`
      + hints([["⌘N", "new routine"], ["esc", "back"]]);
  } else {
    const rows = R.list.map((c, i) => {
      const cur = i === R.sel, label = esc(c.label || "Reminder");
      if (cur && R.mode === "delete") {
        return `<div class="drt" aria-selected="true" data-i="${i}"><span class="tt"><span style="font-size:14px">Delete <b>${label}</b>?</span></span>
          <span class="acts"><button class="dbtn warn" data-r="delete-yes">Delete</button><button class="dbtn" data-r="list">Cancel</button></span></div>`;
      }
      const next = c.enabled ? (c.next_run_at && !c.run_once ? "Next: " + describeDay(new Date(c.next_run_at)) : "") : "Paused";
      const tail = cur && R.mode === "actions"
        ? `<span class="acts"><button class="dbtn" data-r="run">${PLAY_ICON}Run now</button><button class="dbtn" data-r="edit">Edit<kbd>⌘E</kbd></button><button class="dbtn warn" data-r="delete">Delete<kbd>⌘⌫</kbd></button></span>`
        : `<button class="dswitch" role="switch" aria-checked="${c.enabled}" aria-label="${label} on" data-r="toggle" data-i="${i}"></button>`;
      return `<div class="drt${c.enabled ? "" : " off"}" role="option" aria-selected="${cur}" data-i="${i}"><span class="ic">${CLOCK_ICON}</span>
        <span class="tt"><b>${label}</b><span>${esc(c.prompt)}</span><small>${esc([routineWhen(c.schedule), next].filter(Boolean).join(" · "))}</small></span>${tail}</div>`;
    }).join("");
    bar.innerHTML = head + err + `<div class="dlist" id="rlist" role="listbox" aria-label="Routines" style="padding-top:8px">${rows}</div>`
      + hints(R.mode === "actions" ? [["⌘E", "edit"], ["tab", "hide actions"], ["esc", "back"]] : [["↑↓", "move"], ["space", "on or off"], ["tab", "actions"], ["⌘N", "new routine"], ["⌘G", "grants"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = routineClick;
}

export function routineClick(e) {
  if (S.view !== "routines") return;
  const btn = e.target.closest("[data-r]");
  const row = e.target.closest(".drt[data-i]");
  if (btn?.dataset.i) R.sel = Number(btn.dataset.i);
  if (btn) return routineAction(btn.dataset.r);
  if (row) {
    R.sel = Number(row.dataset.i);
    R.mode = "actions";
    drawRoutines();
  }
}

export async function routineAction(a) {
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
      c.enabled = await invoke("toggle_cron", { id: c.id });
    } else if (a === "delete-yes") {
      await invoke("delete_cron", { id: c.id });
      R.list = R.list.filter(x => x.id !== c.id);
    } else if (a === "run") {
      const session = await invoke("run_cron", { id: c.id });
      return openSession(session);
    }
    R.error = "";
  } catch (e) {
    R.error = String(e);
  }
  R.mode = "list";
  if (S.view === "routines") drawRoutines();
}

export function routineKeys(e, cmd) {
  if (R.mode === "delete") {
    if (e.key === "Enter") { e.preventDefault(); routineAction("delete-yes"); }
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    R.sel = Math.max(0, Math.min(R.list.length - 1, R.sel + (e.key === "ArrowDown" ? 1 : -1)));
    R.mode = "list";
    drawRoutines();
  } else if (e.key === "Tab" && R.list.length) {
    e.preventDefault();
    routineAction(R.mode === "actions" ? "list" : "actions");
  } else if (e.key === " " && R.mode === "list" && R.list.length) {
    e.preventDefault();
    routineAction("toggle");
  } else if (cmd && e.key === "Backspace" && R.list.length) {
    e.preventDefault();
    routineAction("delete");
  } else if (e.key === "Enter" && R.mode === "actions") {
    e.preventDefault();
    routineAction("run");
  }
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
  return isNaN(at) ? "" : at.toISOString();
}

export function renderRoutineNew() {
  S.view = "rnew";
  const today = new Date();
  Object.assign(NR, { edit: null, label: "", prompt: "", repeat: "day", days: [1], time: "08:00", cron: "", error: "", busy: false,
    date: `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}` });
  drawRoutineNew();
  $("#nrlabel").focus();
}

export const pad2 = n => String(n).padStart(2, "0");
export const dateValue = d => `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;

/// The form fields for `schedule`, the shapes routineSchedule writes read
/// back. A place trigger stays as it is, anything else shows as cron.
export function routineFields(schedule) {
  if (/^(arrive|leave):/.test(schedule)) return { repeat: "place" };
  if (/^\d{4}-\d\d-\d\dT/.test(schedule)) {
    const at = new Date(schedule);
    if (!isNaN(at)) return { repeat: "once", date: dateValue(at), time: `${pad2(at.getHours())}:${pad2(at.getMinutes())}` };
  }
  const p = schedule.trim().split(/\s+/);
  const cron = { repeat: "cron", cron: schedule };
  if (p.length !== 5 || p[2] !== "*" || p[3] !== "*" || !/^\d+$/.test(p[0])) return cron;
  if (p[1] === "*") return p[4] === "*" ? { repeat: "hour", time: `00:${pad2(p[0])}` } : cron;
  if (!/^\d+$/.test(p[1])) return cron;
  const time = `${pad2(p[1])}:${pad2(p[0])}`;
  if (p[4] === "*") return { repeat: "day", time };
  if (p[4] === "1-5") return { repeat: "weekdays", time };
  if (/^[0-6](,[0-6])*$/.test(p[4])) return { repeat: "week", time, days: p[4].split(",").map(Number) };
  return cron;
}

/// Edits routine `c`'s name and schedule. What it does stays as it is.
export function renderRoutineEdit(c) {
  S.view = "rnew";
  Object.assign(NR, { edit: c, label: c.label, prompt: c.prompt, repeat: "day", days: [1], time: "08:00", cron: "", error: "", busy: false,
    date: dateValue(new Date()) }, routineFields(c.schedule));
  drawRoutineNew();
  $("#nrlabel").focus();
}

export function drawRoutineNew() {
  const chip = (k, l) => `<button class="dchip" aria-pressed="${NR.repeat === k}" data-rep="${k}">${l}</button>`;
  const days = CRON_DAYS.map(d => `<button class="dchip day" aria-pressed="${NR.days.includes(d)}" data-day="${d}">${DAY_NAMES[d]}</button>`).join("");
  const time = `<label class="dfield" style="max-width:140px"><span>${NR.repeat === "hour" ? "Minute past" : "Time"}</span><input id="nrtime" type="time" value="${esc(NR.time)}"></label>`;
  const when = NR.repeat === "cron"
    ? `<label class="dfield"><span>Cron expression</span><input id="nrcron" class="mono" style="text-transform:none" placeholder="30 7 * * 1-5" value="${esc(NR.cron)}" spellcheck="false"></label>
       <span class="dmuted" id="nrcronsays" style="margin-top:-6px"></span>`
    : NR.repeat === "once"
      ? `<div style="display:grid;grid-template-columns:1fr 1fr;gap:10px"><label class="dfield"><span>Date</span><input id="nrdate" type="date" value="${esc(NR.date)}"></label>${time.replace(' style="max-width:140px"', "")}</div>`
      : (NR.repeat === "week" ? `<div class="dchips">${days}</div>` : "") + time;
  const missing = NR.error === "prompt", edit = NR.edit;
  const text = t => `<div class="dmuted" style="display:block;font-size:13.5px;color:var(--fg)">${esc(t)}</div>`;
  const task = edit
    ? `<div class="dfield"><span>What Beep does</span>${text(NR.prompt)}</div>`
    : `<label class="dfield${missing ? " err" : ""}"><span>${missing ? "Tell Beep what to do" : "What should Beep do?"}</span>
        <textarea id="nrprompt" rows="2" placeholder="Summarise my calendar and the weather">${esc(NR.prompt)}</textarea></label>`;
  const schedule = NR.repeat === "place"
    ? `<div class="dfield"><span>Starts</span>${text(routineWhen(edit.schedule))}<span>Change the place in a chat.</span></div>`
    : `<div class="dfield"><span>Repeat</span><div class="dchips">${chip("day", "Every day")}${chip("weekdays", "Weekdays")}${chip("week", "Weekly")}${chip("hour", "Every hour")}${chip("once", "Once")}${chip("cron", "Cron")}</div></div>
      ${when}`;
  const go = edit ? "Save" : "Create";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>${edit ? "Edit routine" : "New routine"}</b><small>${edit ? esc(edit.label || "Reminder") : "Runs on this account, phone or computer off"}</small></span></div>
    <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none;align-content:start">
      <label class="dfield"><span>Name</span><input id="nrlabel" placeholder="Morning briefing" value="${esc(NR.label)}"></label>
      ${task}
      ${schedule}
      ${NR.error && !missing ? `<div class="derr">${esc(NR.error)}</div>` : ""}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" id="nrgo"${NR.busy ? " disabled" : ""}>${go}<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" id="nrno">Cancel</button>
        <span class="dmuted" style="margin-left:auto">${edit ? "To change what it does, ask Beep in a chat" : "Or just tell Beep in a chat"}</span></div>
    </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, go.toLowerCase()], ["esc", "cancel"]])}`;
  const bind = (id, key) => $(id)?.addEventListener("input", e => { NR[key] = e.target.value; if (id === "#nrcron") cronSays(); });
  bind("#nrlabel", "label"); bind("#nrprompt", "prompt"); bind("#nrtime", "time"); bind("#nrdate", "date"); bind("#nrcron", "cron");
  cronSays();
  bar.onclick = e => {
    if (S.view !== "rnew") return;
    const rep = e.target.closest("[data-rep]"), day = e.target.closest("[data-day]");
    if (rep) { NR.repeat = rep.dataset.rep; drawRoutineNew(); }
    else if (day) {
      const d = Number(day.dataset.day);
      NR.days = NR.days.includes(d) ? NR.days.filter(x => x !== d) : [...NR.days, d];
      day.setAttribute("aria-pressed", NR.days.includes(d));
    } else if (e.target.closest("#nrgo")) createRoutine();
    else if (e.target.closest("#nrno")) renderRoutines();
  };
}

export function cronSays() {
  const el = $("#nrcronsays");
  if (!el) return;
  const said = NR.cron.trim() && routineWhen(NR.cron.trim());
  el.textContent = said && said !== NR.cron.trim() ? said : "";
}

export async function createRoutine() {
  if (NR.busy) return;
  if (NR.edit) return saveRoutine();
  const prompt = NR.prompt.trim(), schedule = routineSchedule();
  NR.error = !prompt ? "prompt" : !schedule ? (NR.repeat === "week" ? "Pick at least one day" : "Set when it runs") : "";
  if (NR.error) return drawRoutineNew();
  NR.busy = true;
  try {
    await invoke("create_cron", { label: NR.label.trim() || prompt.slice(0, 40), prompt, schedule });
    if (S.view === "rnew") renderRoutines();
  } catch (e) {
    NR.error = String(e);
    NR.busy = false;
    if (S.view === "rnew") drawRoutineNew();
  }
}

/// Whether two schedules are the same, a one-off in the same minute
/// written another way included.
export function sameSchedule(a, b) {
  if (a === b) return true;
  const iso = /^\d{4}-\d\d-\d\dT/;
  // The form keeps minutes only.
  const minute = s => Math.floor(new Date(s).getTime() / 60000);
  return iso.test(a) && iso.test(b) && minute(a) === minute(b);
}

/// Saves the name and schedule of the routine being edited, sending only
/// what changed.
export async function saveRoutine() {
  const c = NR.edit;
  const schedule = NR.repeat === "place" ? c.schedule : routineSchedule();
  NR.error = !schedule ? (NR.repeat === "week" ? "Pick at least one day" : "Set when it runs") : "";
  if (NR.error) return drawRoutineNew();
  const label = NR.label.trim();
  const changed = { label: label && label !== c.label ? label : null, schedule: sameSchedule(schedule, c.schedule) ? null : schedule };
  if (changed.label == null && changed.schedule == null) return renderRoutines();
  NR.busy = true;
  try {
    await invoke("update_cron", { id: c.id, ...changed });
    if (S.view === "rnew") renderRoutines();
  } catch (e) {
    NR.error = String(e);
    NR.busy = false;
    if (S.view === "rnew") drawRoutineNew();
  }
}

// ---------- Routine grants ----------

// What routines may do without asking (ListGrants). mode: list | confirm
export const GR = { list: [], sel: 0, mode: "list", error: "", loaded: false };
export const SHIELD_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z"/></svg>';

/// "40 min", "5 h" or "6 d".
export function span(ms) {
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
  try {
    GR.list = await invoke("list_grants");
    GR.loaded = true;
    GR.error = "";
  } catch (e) {
    GR.error = String(e);
  }
  if (S.view === "grants") drawGrants();
  else if (S.view === "routines") drawRoutines();
}

export function drawGrants() {
  GR.sel = Math.max(0, Math.min(GR.sel, GR.list.length - 1));
  const head = `<div class="dhead"><span class="tt"><b>Routine grants</b><small>${GR.loaded && !GR.list.length ? "Nothing granted" : "What routines may do without asking. Each ends on its own."}</small></span></div>`;
  const err = GR.error ? `<div class="derr" style="padding:8px 18px 0">${esc(GR.error)}</div>` : "";
  if (GR.loaded && !GR.list.length) {
    bar.innerHTML = head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${SHIELD_ICON}</span>
      <b style="font-size:14px">No routine grants.</b><span class="dmuted" style="display:block">When a routine needs to act without asking, Beep asks you once in a chat.</span></div>` + hints([["esc", "back"]]);
  } else {
    const now = Date.now();
    const rows = GR.list.map((g, i) => {
      const on = i === GR.sel, short = g.action.replace(/ for a routine$/, "");
      if (on && GR.mode === "confirm") {
        return `<div class="drt" aria-selected="true" data-i="${i}"><span class="tt"><span style="font-size:14px">Revoke? <b>${esc(g.routine)}</b> will ask before each ${esc(short)} again.</span></span>
          <span class="acts"><button class="dbtn warn" data-g="revoke-yes">Revoke</button><button class="dbtn" data-g="list">Cancel</button></span></div>`;
      }
      const per = g.per_run === 1 ? "1 each run" : `up to ${g.per_run} each run`;
      const ends = g.expires_at > now ? `Ends in ${span(g.expires_at - now)}` : "Ended";
      const used = g.last_used_at ? `Last used ${span(now - g.last_used_at)} ago` : "Not used yet";
      const acts = on ? `<span class="acts"><button class="dbtn warn" data-g="revoke">Revoke<kbd>⌘⌫</kbd></button></span>` : "";
      const kv = on && g.fields.length ? `<span class="dgkv">${g.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</span>` : "";
      return `<div class="drt" role="option" aria-selected="${on}" data-i="${i}" style="align-items:flex-start"><span class="dapp" style="margin-top:2px">${esc((g.app || "?")[0].toUpperCase())}</span>
        <span class="tt"><b>${esc(g.routine)}</b><span>${esc(`${g.app}: ${short}`)}, ${per}</span><small>${ends} · ${used}</small>${kv}</span>${acts}</div>`;
    }).join("");
    bar.innerHTML = head + err + `<div class="dlist" id="glist" role="listbox" aria-label="Routine grants" style="padding-top:8px">${rows}</div>`
      + hints(GR.mode === "confirm" ? [[ICON.ret, "revoke"], ["esc", "cancel"]] : [["↑↓", "move"], ["⌘⌫", "revoke"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = grantClick;
}

export function grantClick(e) {
  if (S.view !== "grants") return;
  const btn = e.target.closest("[data-g]"), row = e.target.closest(".drt[data-i]");
  if (row) GR.sel = Number(row.dataset.i);
  if (btn) return grantAction(btn.dataset.g);
  if (row && GR.mode === "list") drawGrants();
}

export async function grantAction(a) {
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

export function grantKeys(e, cmd) {
  if (GR.mode === "confirm") {
    if (e.key === "Enter") { e.preventDefault(); grantAction("revoke-yes"); }
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    GR.sel = Math.max(0, Math.min(GR.list.length - 1, GR.sel + (e.key === "ArrowDown" ? 1 : -1)));
    drawGrants();
  } else if (cmd && e.key === "Backspace" && GR.list.length) {
    e.preventDefault();
    grantAction("revoke");
  }
}

