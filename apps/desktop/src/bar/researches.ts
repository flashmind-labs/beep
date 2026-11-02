import { failed, grow, newTurn } from "./chat.ts";
import { when } from "./chats.ts";
import { $, bar, esc, ICON, invoke, S } from "./core.ts";
import { hints, refreshHints } from "./hints.ts";
import { notice } from "./media.ts";

// ---------- Research ----------

/// Chats started as researches, which open in their own window.
export const RESEARCHES = new Set();
export async function loadResearches() {
  const ids = await invoke("research_sessions").catch(() => []);
  for (const id of ids) RESEARCHES.add(id);
}

/// Starts a research on `topic` in its own window.
export async function research(topic) {
  topic = topic.trim();
  if (!topic) return;
  try {
    const id = await invoke("research_begin", { topic });
    RESEARCHES.add(id);
    const ask = $("#ask");
    if (ask && ask.value.trim() === topic) { ask.value = ""; grow(ask); refreshHints(); }
  } catch (err) {
    if (S.view === "research") { RL.error = String(err); return drawResearches(); }
    failed(newTurn(""), String(err));
  }
}

/// The bar's Research view: every research on the account, newest first,
/// from the server, so ones started on another computer show too. The field
/// on top starts a new one; Return on a row opens its window.
export const RL = { list: [], loaded: false, sel: 0, mode: "list", error: "" };
export const RSTATUS = {
  planning: "Waiting for your go", researching: "Researching", reviewing: "Reviewing",
  writing: "Writing", done: "Done", failed: "Failed", stopped: "Stopped", interrupted: "Interrupted",
};
export const RUNNING = new Set(["researching", "reviewing", "writing"]);

export function renderResearches(topic = "") {
  S.view = "research";
  invoke("close_group").catch(() => {});
  Object.assign(RL, { mode: "list", error: "", sel: 0 });
  drawResearches();
  const field = $("#rtopic");
  if (field) { field.value = topic; field.focus(); }
  loadResearchList();
}

export async function loadResearchList() {
  try {
    RL.list = await invoke("list_researches");
    for (const r of RL.list) RESEARCHES.add(r.session);
    RL.loaded = true;
    RL.error = "";
  } catch (e) {
    RL.error = String(e);
  }
  if (S.view === "research") drawResearches();
}

export function researchLine(r) {
  const parts = [RSTATUS[r.status] ?? r.status];
  if (r.round > 1) parts.unshift(`Round ${r.round}`);
  if (r.sources) parts.push(`${r.sources} source${r.sources === 1 ? "" : "s"}`);
  parts.push(when(r.finished_at ?? r.started_at));
  return parts.join(" · ");
}

export function drawResearches() {
  const typed = $("#rtopic")?.value ?? "";
  const focused = document.activeElement?.id === "rtopic";
  RL.sel = Math.max(0, Math.min(RL.sel, RL.list.length - 1));
  const running = RL.list.filter(r => RUNNING.has(r.status)).length;
  const waiting = RL.list.filter(r => r.status === "planning").length;
  const sum = !RL.loaded ? "" : !RL.list.length ? "Nothing researched yet"
    : [running && `${running} running`, waiting && `${waiting} waiting`].filter(Boolean).join(", ") || `${RL.list.length} researched`;
  const head = `<div class="dhead"><span class="tt"><b>Research</b><small>${esc(sum)}</small></span></div>`;
  const field = `<div class="dinput"><input id="rtopic" placeholder="What should the team research?" autocomplete="off" spellcheck="false"><kbd>⌘R</kbd></div>`;
  const err = RL.error ? `<div class="derr" style="padding:8px 18px 0">${esc(RL.error)}</div>` : "";
  if (RL.loaded && !RL.list.length) {
    bar.innerHTML = field + head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${ICON.browser}</span>
      <b style="font-size:14px">No research yet.</b><span class="dmuted" style="display:block">A team of agents digs into a question and writes it up as a paper.</span></div>`
      + hints([[ICON.ret, "research"], ["esc", "back"]]);
  } else {
    const rows = RL.list.map((r, i) => {
      const cur = i === RL.sel && !typed.trim();
      const ic = RUNNING.has(r.status) ? '<span class="dspin"></span>'
        : r.status === "planning" ? '<span class="ddot" style="background:var(--coral)"></span>'
        : r.status === "done" ? `<span class="dok">${ICON.check}</span>` : ICON.doc;
      const tail = cur && RL.mode === "actions" && r.sources
        ? '<span class="acts"><button class="dbtn" data-rl="pdf">Export PDF</button></span>' : "";
      const off = ["failed", "stopped", "interrupted"].includes(r.status);
      return `<div class="drt${off ? " off" : ""}" role="option" aria-selected="${cur}" data-i="${i}"><span class="ic">${ic}</span>
        <span class="tt"><b>${esc(r.topic)}</b><small>${esc(researchLine(r))}</small></span>${tail}</div>`;
    }).join("");
    const keys = typed.trim() ? [[ICON.ret, "research"], ["esc", "back"]]
      : RL.mode === "actions" ? [["tab", "hide actions"], ["esc", "back"]]
      : [["↑↓", "move"], [ICON.ret, "open"], ["tab", "actions"], ["esc", "back"]];
    bar.innerHTML = field + head + err + `<div class="dlist" role="listbox" aria-label="Research" style="padding-top:8px">${rows}</div>` + hints(keys);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  const input = $("#rtopic");
  input.value = typed;
  if (focused || !RL.list.length) input.focus();
  input.oninput = () => { const at = input.selectionStart; drawResearches(); $("#rtopic").setSelectionRange(at, at); };
  bar.onclick = researchClick;
}

export function researchClick(e) {
  if (S.view !== "research") return;
  const btn = e.target.closest("[data-rl]");
  const row = e.target.closest(".drt[data-i]");
  if (row) RL.sel = Number(row.dataset.i);
  if (btn) return researchListAction(btn.dataset.rl);
  if (row) return researchListAction("open");
}

export async function researchListAction(a) {
  const r = RL.list[RL.sel];
  if (!r) return;
  try {
    if (a === "open") return await invoke("open_research", { session: r.session });
    if (a === "pdf") {
      const to = await invoke("export_research", { session: r.session });
      RL.error = "";
      RL.mode = "list";
      drawResearches();
      return notice(`Saved ${to.split("/").pop()} to Downloads`, { path: to });
    }
  } catch (e) {
    RL.error = String(e);
    drawResearches();
  }
}

/// Cmd-R: researches what is typed, and otherwise opens the Research view.
export function researchShortcut() {
  const typed = (S.view === "chat" ? $("#ask") : S.view === "research" ? $("#rtopic") : null)?.value.trim();
  if (typed) return research(typed);
  if (S.view !== "research") return renderResearches();
}

export function researchKeys(e) {
  const typed = $("#rtopic")?.value.trim();
  if (e.key === "Enter" && !e.isComposing) {
    e.preventDefault();
    return typed ? research(typed) : researchListAction("open");
  }
  if (typed) return;
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    RL.sel = Math.max(0, Math.min(RL.list.length - 1, RL.sel + (e.key === "ArrowDown" ? 1 : -1)));
    RL.mode = "list";
    drawResearches();
  } else if (e.key === "Tab") {
    e.preventDefault();
    RL.mode = RL.mode === "actions" ? "list" : "actions";
    drawResearches();
  }
}

