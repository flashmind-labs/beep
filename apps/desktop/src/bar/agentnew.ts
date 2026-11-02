import { AG, agentKind, byPreference, CAP_NAMES, loadAgentHosts, openAgent, renderAgents } from "./agents.ts";
import { $, bar, esc, ICON, invoke, S } from "./core.ts";
import { hints } from "./hints.ts";
import { back } from "./keys.ts";

// ----- Start form -----

/// The start form with `prompt`; `from` is where Esc goes back to.
/// `at` presets the machine and folder, as a project's "Start an agent here" does.
export function renderAgentNew(prompt = "", from = "chat", at = null) {
  const chat = S.view === "chat" ? S.session : null;
  S.view = "agentnew";
  AG.back = from;
  AG.form = { prompt, machine: at?.machine || "auto", agent: null, cwd: at?.cwd || "", chat };
  AG.error = "";
  AG.busy = false;
  drawAgentNew();
  $("#agprompt")?.focus();
  loadAgentHosts();
  invoke("agent_sessions").then(list => { AG.sessions = list; if (S.view === "agentnew") drawFolders(); }).catch(() => {});
}

export function formHost() { return AG.hosts?.find(m => m.machine_id === AG.form.machine) ?? null; }
/// The agents the chosen machine offers; for Auto, any machine's.
export function formAgents() {
  const host = formHost();
  const all = host ? host.agents : [...new Set((AG.hosts ?? []).flatMap(m => m.agents))];
  return all.sort(byPreference);
}

export function drawAgentNew() {
  const f = AG.form, hosts = AG.hosts ?? [], host = formHost();
  const agents = formAgents();
  if (f.agent && !agents.includes(f.agent)) f.agent = null;
  const chip = (attr, val, on, label, sub) => `<button class="dchip" aria-pressed="${on}" ${attr}="${esc(val)}"${sub ? ` title="${esc(sub)}"` : ""}>${label}</button>`;
  const caps = m => m.capabilities.map(c => CAP_NAMES[c] || c);
  const machines = [chip("data-agm", "auto", f.machine === "auto", "Auto", "Beep picks from what each machine has"),
    ...hosts.map(m => chip("data-agm", m.machine_id, f.machine === m.machine_id, esc(m.machine_name), caps(m).join(", ")))].join("");
  const machineNote = !AG.hosts ? '<span class="dspin"></span> Looking for machines…'
    : !hosts.length ? "None of your machines online offers a coding agent. Run beep-node on one with Claude Code, Codex, Gemini CLI, pi, flsh or OpenCode."
    : host ? (caps(host).length ? `Has ${esc(caps(host).join(", "))}` : "No special hardware or apps reported")
    : "Beep picks the machine with what the task needs, such as Xcode, a GPU or Blender, else your environment or the least busy one.";
  const agentChips = [chip("data-aga", "", !f.agent, "Any"), ...agents.map(a => chip("data-aga", a, f.agent === a, esc(agentKind(a).name)))].join("");
  const place = host ? (host.workspace || host.home || "Home folder") : "Workspace or home folder";
  const missing = AG.error === "prompt";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Start a coding agent</b><small>It runs on your machine; follow it and answer it here</small></span>
      <button class="dbtn quiet" id="agList">All sessions</button></div>
    <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none;align-content:start">
      <label class="dfield${missing ? " err" : ""}"><span>${missing ? "Say what the agent should do" : "What should it do?"}</span>
        <textarea id="agprompt" rows="3" placeholder="Fix the failing login test">${esc(f.prompt)}</textarea></label>
      <div class="dfield"><span>Machine</span><div class="dchips">${machines}</div><span class="note">${machineNote}</span><span class="note why" id="agwhy"${hosts.length && !host ? "" : " hidden"}>${pickNote()}</span></div>
      <label class="dfield"><span>Folder</span><input id="agcwd" list="agfolders" spellcheck="false" placeholder="${esc(place)}" value="${esc(f.cwd)}" style="font-family:var(--mono);font-size:13px"><datalist id="agfolders"></datalist></label>
      <div class="dfield"><span>Agent</span><div class="dchips">${agentChips}</div></div>
      ${AG.error && !missing ? `<div class="derr">${esc(AG.error)}</div>` : ""}
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" id="aggo"${AG.busy || !hosts.length ? " disabled" : ""}>${AG.busy ? '<span class="dspin"></span>Starting' : "Start"}<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" id="agno">Cancel</button>
        <span class="dmuted" style="margin-left:auto">Or ask Beep in a chat to start one</span></div>
    </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, "start"], ["esc", "cancel"]])}`;
  drawFolders();
  $("#agprompt").addEventListener("input", e => { f.prompt = e.target.value; previewAuto(); });
  $("#agcwd").addEventListener("input", e => { f.cwd = e.target.value; previewAuto(); });
  previewAuto();
  bar.onclick = e => {
    if (S.view !== "agentnew") return;
    const m = e.target.closest("[data-agm]"), a = e.target.closest("[data-aga]");
    if (m) { f.machine = m.dataset.agm; drawAgentNew(); }
    else if (a) { f.agent = a.dataset.aga || null; drawAgentNew(); }
    else if (e.target.closest("#aggo")) startAgent();
    else if (e.target.closest("#agno")) back();
    else if (e.target.closest("#agList")) renderAgents();
  };
}

/// What Auto would pick for the form as it stands: the server's answer for
/// `key`, else nothing while it is asked.
export const PICK = { key: "", pick: null, error: "", timer: 0 };

/// The line under the machines when Auto is on: where it would start now.
export function pickNote() {
  const f = AG.form;
  if (!f?.prompt.trim()) return "";
  if (PICK.key !== pickKey()) return '<span class="dspin"></span> Choosing a machine…';
  if (PICK.error) return esc(PICK.error);
  if (!PICK.pick) return "";
  return `Auto would pick <b>${esc(PICK.pick.machine_name)}</b>. ${esc(PICK.pick.why)}`;
}

export function pickKey() {
  const f = AG.form;
  return JSON.stringify([f.prompt.trim(), f.cwd.trim(), f.agent]);
}

/// Asks the server where Auto would start the form, once typing pauses.
/// Starting nothing; the answer only fills the note.
export function previewAuto() {
  clearTimeout(PICK.timer);
  const f = AG.form, note = () => $("#agwhy");
  const auto = S.view === "agentnew" && f.machine === "auto" && AG.hosts?.length;
  if (!auto) return;
  if (note()) note().innerHTML = pickNote();
  if (!f.prompt.trim() || PICK.key === pickKey()) return;
  PICK.timer = setTimeout(async () => {
    const key = pickKey();
    let pick = null, error = "";
    try {
      pick = await invoke("agent_preview", { agent: f.agent, cwd: f.cwd.trim() || null, prompt: f.prompt.trim() });
    } catch (e) {
      error = String(e).replace(/^bad request: /, "");
    }
    if (key !== pickKey()) return;
    Object.assign(PICK, { key, pick, error });
    if (S.view !== "agentnew") return;
    if (note()) {
      note().innerHTML = pickNote();
      note().hidden = !pick && !error;
    }
  }, 450);
}

/// Folder suggestions: the machine's workspace and home, then where its sessions ran.
export function drawFolders() {
  const list = $("#agfolders");
  if (!list) return;
  const host = formHost(), hosts = host ? [host] : AG.hosts ?? [];
  const ids = new Set(hosts.map(m => m.machine_id));
  const seen = [...hosts.flatMap(m => [m.workspace, m.home]),
    ...AG.sessions.filter(s => ids.has(s.machine_id)).sort((a, b) => b.last_activity - a.last_activity).map(s => s.cwd)];
  list.innerHTML = [...new Set(seen.filter(Boolean))].slice(0, 12).map(p => `<option value="${esc(p)}"></option>`).join("");
}

export async function startAgent() {
  const f = AG.form;
  if (AG.busy || S.view !== "agentnew") return;
  if (!f.prompt.trim()) { AG.error = "prompt"; drawAgentNew(); return $("#agprompt")?.focus(); }
  AG.busy = true;
  AG.error = "";
  drawAgentNew();
  try {
    const started = await invoke("agent_start", {
      machineId: f.machine === "auto" ? null : f.machine,
      agent: f.agent,
      cwd: f.cwd.trim() || null,
      prompt: f.prompt.trim(),
      chat: f.chat,
    });
    AG.busy = false;
    if (S.view !== "agentnew") return;
    // Back from the session goes where the form came from.
    openAgent(started.session, { why: started.why, back: AG.back });
  } catch (e) {
    AG.busy = false;
    AG.error = String(e).replace(/^bad request: /, "");
    if (S.view === "agentnew") drawAgentNew();
  }
}

