import { $, bar, esc, ICON, invoke, S } from "./core.ts";
import { hints } from "./hints.ts";
import { MONTHS } from "./memories.ts";

// ---------- Skills ----------

// How the user, or a group, likes a task done. Beep writes them on its own;
// here they can be read, edited, paused, undone and deleted.
// mode: list | actions | delete | view | edit | vdelete
export const K = { list: [], sel: 0, mode: "list", error: "", loaded: false };
export const SKILL_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 19l9-9"/><path d="M14 4l1.2 2.8L18 8l-2.8 1.2L14 12l-1.2-2.8L10 8l2.8-1.2z"/><path d="M19 13l.6 1.4L21 15l-1.4.6L19 17l-.6-1.4L17 15l1.4-.6z"/></svg>';

/// "Used today", "Used yesterday" or "Used 8 Oct".
export function skillUsed(iso) {
  if (!iso) return "Not used yet";
  const at = new Date(iso);
  if (isNaN(at)) return "";
  const day = d => new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const ago = Math.round((day(new Date()) - day(at)) / 86400000);
  return "Used " + (ago <= 0 ? "today" : ago === 1 ? "yesterday" : `${at.getDate()} ${MONTHS[at.getMonth()]}`);
}
export function skillMeta(k) {
  if (!k.enabled) return "Paused";
  const uses = k.uses === 1 ? "1 use" : `${k.uses} uses`;
  return [skillUsed(k.last_used), uses, k.edited && "edited by you"].filter(Boolean).join(" · ");
}

export function renderSkills() {
  S.view = "skills";
  invoke("close_group").catch(() => {});
  Object.assign(K, { mode: "list", error: "" });
  drawSkills();
  loadSkills();
}

export async function loadSkills() {
  try {
    K.list = await invoke("list_skills");
    K.loaded = true;
    K.error = "";
  } catch (e) {
    K.error = String(e);
  }
  if (S.view === "skills" && (K.mode === "list" || K.mode === "actions")) drawSkills();
}

export function drawSkills() {
  K.sel = Math.max(0, Math.min(K.sel, K.list.length - 1));
  if (K.mode === "view" || K.mode === "edit" || K.mode === "vdelete") return drawSkill();
  const paused = K.list.filter(k => !k.enabled).length;
  const sum = !K.loaded ? "" : !K.list.length ? "None yet" : [`${K.list.length} ${K.list.length === 1 ? "skill" : "skills"}`, paused && `${paused} paused`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Skills</b><small>${sum}</small></span></div>`;
  const err = K.error ? `<div class="derr" style="padding:8px 18px 0">${esc(K.error)}</div>` : "";
  if (K.loaded && !K.list.length) {
    bar.innerHTML = head + err + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--dim);display:grid">${SKILL_ICON}</span>
      <b style="font-size:14px">No skills yet.</b><span class="dmuted" style="display:block;max-width:44ch">When Beep works out how you like something done, like booking a court or a weekly summary, it writes it down here and does it your way next time.</span></div>`
      + hints([["esc", "back"]]);
  } else {
    let owner, html = "";
    K.list.forEach((k, i) => {
      if (k.group_id !== owner || i === 0) {
        owner = k.group_id;
        html += `${i ? "</div>" : ""}<div class="dsec flat" style="padding:8px 10px 4px">${esc(k.group_name || (k.group_id ? "Group" : "Yours"))}</div><div class="dlist" style="padding:0">`;
      }
      const cur = i === K.sel, name = esc(k.name);
      if (cur && K.mode === "delete") {
        html += `<div class="drt" aria-selected="true" data-i="${i}"><span class="tt"><span style="font-size:14px">Delete <b>${name}</b>?</span></span>
          <span class="acts"><button class="dbtn warn" data-k="delete-yes">Delete</button><button class="dbtn" data-k="list">Cancel</button></span></div>`;
        return;
      }
      const tail = cur && K.mode === "actions"
        ? `<span class="acts"><button class="dbtn" data-k="view">Open<kbd>${ICON.ret}</kbd></button>${k.can_undo ? `<button class="dbtn" data-k="undo">Undo<kbd>⌘Z</kbd></button>` : ""}<button class="dbtn warn" data-k="delete">Delete<kbd>⌘⌫</kbd></button></span>`
        : `<button class="dswitch" role="switch" aria-checked="${k.enabled}" aria-label="Use ${name}" data-k="toggle" data-i="${i}"></button>`;
      html += `<div class="drt${k.enabled ? "" : " off"}" role="option" aria-selected="${cur}" data-i="${i}" style="cursor:pointer"><span class="ic">${SKILL_ICON}</span>
        <span class="tt"><b>${name}</b><span>${esc(k.description)}</span><small>${esc(skillMeta(k))}</small></span>${tail}</div>`;
    });
    bar.innerHTML = head + err + `<div class="dlist" id="klist" role="listbox" aria-label="Skills" style="padding-top:4px;overflow:auto;scrollbar-width:none">${html}</div></div>`
      + hints(K.mode === "actions" ? [[ICON.ret, "open"], ["⌘Z", "undo"], ["⌘⌫", "delete"], ["tab", "hide actions"], ["esc", "back"]]
        : [["↑↓", "move"], [ICON.ret, "open"], ["space", "on or off"], ["tab", "actions"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = skillClick;
}

/// One skill: its instructions, or the editor.
export function drawSkill() {
  const k = K.list[K.sel];
  if (!k) { K.mode = "list"; return drawSkills(); }
  const err = K.error ? `<div class="derr" style="padding:8px 18px 0">${esc(K.error)}</div>` : "";
  if (K.mode === "edit") {
    bar.innerHTML = `<div class="dhead"><span class="tt"><b>Edit skill</b><small>Beep keeps the old version, so you can undo</small></span></div>${err}
      <div class="dform" style="padding-top:12px;overflow:auto;scrollbar-width:none">
        <label class="dfield"><span>Name</span><input id="kname" maxlength="60" value="${esc(k.name)}" spellcheck="false"></label>
        <label class="dfield"><span>When it applies</span><input id="kdesc" maxlength="200" value="${esc(k.description)}"></label>
        <label class="dfield"><span>Instructions</span><textarea id="kbody" rows="12" style="font-size:13px;line-height:19px">${esc(k.body)}</textarea></label>
        <div style="display:flex;gap:8px"><button class="dbtn pri" data-k="save">Save<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" data-k="view">Cancel</button></div>
      </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, "save"], ["esc", "cancel"]])}`;
    bar.onclick = skillClick;
    return $("#kbody")?.focus();
  }
  const confirm = K.mode === "vdelete" ? `<div class="dcard" style="margin:12px 16px 0"><div style="padding:10px 12px;display:grid;gap:8px">
      <b style="font-size:13.5px">Delete this skill?</b><div class="dmuted" style="display:block">Beep forgets how you like "${esc(k.name)}" done. It may learn it again if you keep asking for it.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" data-k="delete-yes">Delete<kbd>${ICON.ret}</kbd></button><button class="dbtn" data-k="view">Cancel</button></div></div></div>` : "";
  const undo = k.can_undo ? `<div class="dmuted" style="border-top:1px solid var(--border);padding-top:10px"><span style="flex:1">${k.edited ? "You changed this last." : "Beep changed this last."}</span><button class="dbtn" data-k="undo">Undo<kbd>⌘Z</kbd></button></div>` : "";
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>${esc(k.name)}</b><small>${esc(k.group_name ? `${k.group_name} · ${k.description}` : k.description)}</small></span>
      <button class="dswitch" role="switch" aria-checked="${k.enabled}" aria-label="Use this skill" data-k="toggle"></button>
      <button class="dbtn" data-k="edit">Edit<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn warn" data-k="vdelete">Delete<kbd>⌘⌫</kbd></button></div>${err}${confirm}
    <div class="dbody" style="padding-top:12px"><div class="dmuted">${esc(skillMeta(k))}</div><div class="reply">${k.html}</div>${undo}</div>`
    + hints(K.mode === "vdelete" ? [[ICON.ret, "delete"], ["esc", "cancel"]] : [["space", "on or off"], [`⌘${ICON.ret}`, "edit"], ...(k.can_undo ? [["⌘Z", "undo"]] : []), ["esc", "back"]]);
  bar.onclick = skillClick;
}

export function skillClick(e) {
  if (S.view !== "skills") return;
  const btn = e.target.closest("[data-k]");
  const row = e.target.closest(".drt[data-i]");
  if (btn?.dataset.i) K.sel = Number(btn.dataset.i);
  if (btn) return skillAction(btn.dataset.k);
  if (row && K.mode !== "delete") {
    K.sel = Number(row.dataset.i);
    skillAction("view");
  }
}

/// Replaces the skill at `K.sel` with what the server sent back.
export function putSkill(k) { K.list[K.sel] = k; }

export async function skillAction(a) {
  const k = K.list[K.sel];
  if (a === "list" || a === "actions" || a === "delete" || a === "view" || a === "edit" || a === "vdelete") {
    if (!k && a !== "list") return;
    K.mode = a;
    K.error = "";
    return drawSkills();
  }
  if (!k) return;
  const inView = K.mode === "view" || K.mode === "edit" || K.mode === "vdelete";
  try {
    if (a === "toggle") {
      putSkill(await invoke("update_skill", { id: k.id, change: { enabled: !k.enabled } }));
    } else if (a === "undo") {
      if (!k.can_undo) return;
      putSkill(await invoke("undo_skill", { id: k.id }));
    } else if (a === "save") {
      const name = $("#kname").value.trim(), description = $("#kdesc").value.trim(), body = $("#kbody").value.trim();
      if (!name || !body) { K.error = name ? "Write what Beep should do." : "Give the skill a name."; return drawSkill(); }
      const change = {};
      if (name !== k.name) change.name = name;
      if (description !== k.description) change.description = description;
      if (body !== k.body) change.body = body;
      if (Object.keys(change).length) putSkill(await invoke("update_skill", { id: k.id, change }));
    } else if (a === "delete-yes") {
      await invoke("delete_skill", { id: k.id });
      K.list = K.list.filter(x => x.id !== k.id);
      K.mode = "list";
      K.error = "";
      return S.view === "skills" && drawSkills();
    }
    K.error = "";
  } catch (e) {
    K.error = String(e);
  }
  K.mode = inView ? "view" : "list";
  if (S.view === "skills") drawSkills();
}

export function skillKeys(e, cmd) {
  const key = e.key.toLowerCase();
  if (K.mode === "edit") {
    if (cmd && e.key === "Enter") { e.preventDefault(); skillAction("save"); }
    return;
  }
  if (K.mode === "delete" || K.mode === "vdelete") {
    if (e.key === "Enter") { e.preventDefault(); skillAction("delete-yes"); }
    return;
  }
  if (!K.list.length) return;
  if (cmd && key === "z") { e.preventDefault(); return skillAction("undo"); }
  if (cmd && e.key === "Backspace") { e.preventDefault(); return skillAction(K.mode === "view" ? "vdelete" : "delete"); }
  if (e.key === " ") { e.preventDefault(); return skillAction("toggle"); }
  if (K.mode === "view") {
    if (cmd && e.key === "Enter") { e.preventDefault(); skillAction("edit"); }
    return;
  }
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    K.sel = Math.max(0, Math.min(K.list.length - 1, K.sel + (e.key === "ArrowDown" ? 1 : -1)));
    K.mode = "list";
    drawSkills();
  } else if (e.key === "Tab") {
    e.preventDefault();
    skillAction(K.mode === "actions" ? "list" : "actions");
  } else if (e.key === "Enter") {
    e.preventDefault();
    skillAction("view");
  }
}

