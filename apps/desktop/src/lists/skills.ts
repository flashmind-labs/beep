import { $$, bar, esc, ICON, invoke, S } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
import { arrowKey, clampSel, confirmKey, confirmRow, dayDiff, emptyPage, errLine, type ListState, loadList, readDay } from "./memories.ts";
import { plural } from "../ui/format.ts";

// ---------- Skills ----------

// How the user, or a group, likes a task done. Beep writes them on its own;
// here they can be read, edited, paused, undone and deleted.

/// A skill as `list_skills` sends it: the server's fields and its body as HTML.
export interface Skill {
  id: string;
  name: string;
  /// One line on when it applies.
  description: string;
  /// The instructions in Markdown, and drawn as HTML.
  body: string;
  html: string;
  /// The group it belongs to; both null for the user's own.
  group_id: string | null;
  group_name: string | null;
  enabled: boolean;
  uses: number;
  /// RFC 3339, as is `updated_at`.
  last_used: string | null;
  updated_at: string;
  /// Whether the user changed it last, rather than Beep.
  edited: boolean;
  can_undo: boolean;
}

/// The changes `update_skill` takes; a field left out stays as it is.
type SkillChange = Partial<Pick<Skill, "name" | "description" | "body" | "enabled">>;

type SkillMode = "list" | "actions" | "delete" | "view" | "edit" | "vdelete";
type SkillAction = SkillMode | "toggle" | "undo" | "save" | "delete-yes";
const MODES = new Set<string>(["list", "actions", "delete", "view", "edit", "vdelete"]);
/// The modes that show one skill rather than the list.
const ONE = new Set<string>(["view", "edit", "vdelete"]);

export const K: ListState<Skill> & { mode: SkillMode } = { list: [], sel: 0, mode: "list", error: "", loaded: false };
export const SKILL_ICON = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 19l9-9"/><path d="M14 4l1.2 2.8L18 8l-2.8 1.2L14 12l-1.2-2.8L10 8l2.8-1.2z"/><path d="M19 13l.6 1.4L21 15l-1.4.6L19 17l-.6-1.4L17 15l1.4-.6z"/></svg>';

/// "Used today", "Used yesterday" or "Used 8 Oct".
export function skillUsed(iso: string | null) {
  if (!iso) return "Not used yet";
  const at = new Date(iso);
  if (isNaN(at.getTime())) return "";
  return "Used " + (dayDiff(at, new Date()) <= 0 ? "today" : readDay(at));
}
export function skillMeta(k: Skill) {
  if (!k.enabled) return "Paused";
  const uses = plural(k.uses, "use", "uses");
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
  await loadList(K, "list_skills");
  if (S.view === "skills" && (K.mode === "list" || K.mode === "actions")) drawSkills();
}

function skillRow(k: Skill, i: number) {
  const cur = i === K.sel, name = esc(k.name);
  if (cur && K.mode === "delete") return confirmRow(i, `Delete <b>${name}</b>?`, "data-k", "Delete");
  const tail = cur && K.mode === "actions"
    ? `<span class="acts"><button class="dbtn" data-k="view">Open<kbd>${ICON.ret}</kbd></button>${k.can_undo ? `<button class="dbtn" data-k="undo">Undo<kbd>⌘Z</kbd></button>` : ""}<button class="dbtn warn" data-k="delete">Delete<kbd>⌘⌫</kbd></button></span>`
    : `<button class="dswitch" role="switch" aria-checked="${k.enabled}" aria-label="Use ${name}" data-k="toggle" data-i="${i}"></button>`;
  return `<div class="drt${k.enabled ? "" : " off"}" role="option" aria-selected="${cur}" data-i="${i}" style="cursor:pointer"><span class="ic">${SKILL_ICON}</span>
        <span class="tt"><b>${name}</b><span>${esc(k.description)}</span><small>${esc(skillMeta(k))}</small></span>${tail}</div>`;
}

export function drawSkills(): void {
  clampSel(K);
  if (ONE.has(K.mode)) return drawSkill();
  const paused = K.list.filter(k => !k.enabled).length;
  const sum = !K.loaded ? "" : !K.list.length ? "None yet" : [plural(K.list.length, "skill", "skills"), paused && `${paused} paused`].filter(Boolean).join(", ");
  const head = `<div class="dhead"><span class="tt"><b>Skills</b><small>${sum}</small></span></div>`;
  if (K.loaded && !K.list.length) {
    bar.innerHTML = head + errLine(K.error) + emptyPage(SKILL_ICON, "No skills yet.", "Beep saves how you like things done here.", "", ";max-width:44ch")
      + hints([["esc", "back"]]);
  } else {
    // Under a heading for each owner: the user, then each group.
    let owner: string | null | undefined, html = "";
    K.list.forEach((k, i) => {
      if (k.group_id !== owner || i === 0) {
        owner = k.group_id;
        html += `${i ? "</div>" : ""}<div class="dsec flat" style="padding:8px 10px 4px">${esc(k.group_name || (k.group_id ? "Group" : "Yours"))}</div><div class="dlist" style="padding:0">`;
      }
      html += skillRow(k, i);
    });
    bar.innerHTML = head + errLine(K.error) + `<div class="dlist" id="klist" role="listbox" aria-label="Skills" style="padding-top:4px;overflow:auto;scrollbar-width:none">${html}</div></div>`
      + hints(K.mode === "actions" ? [[ICON.ret, "open"], ["⌘Z", "undo"], ["⌘⌫", "delete"], ["tab", "hide actions"], ["esc", "back"]]
        : [["↑↓", "move"], [ICON.ret, "open"], ["space", "on or off"], ["tab", "actions"], ["esc", "back"]]);
    bar.querySelector('.drt[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }
  bar.onclick = skillClick;
}

/// One skill: its instructions, or the editor.
export function drawSkill(): void {
  const k = K.list[K.sel];
  if (!k) { K.mode = "list"; return drawSkills(); }
  const err = errLine(K.error);
  bar.onclick = skillClick;
  if (K.mode === "edit") {
    bar.innerHTML = `<div class="dhead"><span class="tt"><b>Edit skill</b><small>Beep keeps the old version, so you can undo</small></span></div>${err}
      <div class="dform" style="padding-top:12px;overflow:auto;scrollbar-width:none">
        <label class="dfield"><span>Name</span><input id="kname" maxlength="60" value="${esc(k.name)}" spellcheck="false"></label>
        <label class="dfield"><span>When it applies</span><input id="kdesc" maxlength="200" value="${esc(k.description)}"></label>
        <label class="dfield"><span>Instructions</span><textarea id="kbody" rows="12" style="font-size:13px;line-height:19px">${esc(k.body)}</textarea></label>
        <div style="display:flex;gap:8px"><button class="dbtn pri" data-k="save">Save<kbd>⌘${ICON.ret}</kbd></button><button class="dbtn" data-k="view">Cancel</button></div>
      </div>${hints([["tab", "next field"], [`⌘${ICON.ret}`, "save"], ["esc", "cancel"]])}`;
    return $$("#kbody").focus();
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
}

export function skillClick(e: MouseEvent) {
  if (S.view !== "skills") return;
  const target = e.target as HTMLElement;
  const btn = target.closest<HTMLElement>("[data-k]");
  const row = target.closest<HTMLElement>(".drt[data-i]");
  if (btn?.dataset.i) K.sel = Number(btn.dataset.i);
  if (btn) return skillAction(btn.dataset.k as SkillAction);
  if (row && K.mode !== "delete") {
    K.sel = Number(row.dataset.i);
    skillAction("view");
  }
}

/// Replaces the skill at `K.sel` with what the server sent back.
export function putSkill(k: Skill) { K.list[K.sel] = k; }

/// The editor's changes to `k`, only what differs; null when a field it
/// needs is empty, after saying so.
function editedSkill(k: Skill): SkillChange | null {
  const value = (id: string) => $$<HTMLInputElement>(id).value.trim();
  const name = value("#kname"), description = value("#kdesc"), body = value("#kbody");
  if (!name || !body) { K.error = name ? "Write what Beep should do." : "Give the skill a name."; return null; }
  const change: SkillChange = {};
  if (name !== k.name) change.name = name;
  if (description !== k.description) change.description = description;
  if (body !== k.body) change.body = body;
  return change;
}

export async function skillAction(a: SkillAction) {
  const k = K.list[K.sel];
  if (MODES.has(a)) {
    if (!k && a !== "list") return;
    K.mode = a as SkillMode;
    K.error = "";
    return drawSkills();
  }
  if (!k) return;
  const inView = ONE.has(K.mode);
  try {
    if (a === "toggle") {
      putSkill(await invoke<Skill>("update_skill", { id: k.id, change: { enabled: !k.enabled } }));
    } else if (a === "undo") {
      if (!k.can_undo) return;
      putSkill(await invoke<Skill>("undo_skill", { id: k.id }));
    } else if (a === "save") {
      const change = editedSkill(k);
      if (!change) return drawSkill();
      if (Object.keys(change).length) putSkill(await invoke<Skill>("update_skill", { id: k.id, change }));
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

export function skillKeys(e: KeyboardEvent, cmd: boolean) {
  if (K.mode === "edit") {
    if (cmd && e.key === "Enter") { e.preventDefault(); skillAction("save"); }
    return;
  }
  if (K.mode === "delete" || K.mode === "vdelete") return confirmKey(e, () => skillAction("delete-yes"));
  if (!K.list.length) return;
  if (cmd && e.key.toLowerCase() === "z") { e.preventDefault(); return skillAction("undo"); }
  if (cmd && e.key === "Backspace") { e.preventDefault(); return skillAction(K.mode === "view" ? "vdelete" : "delete"); }
  if (e.key === " ") { e.preventDefault(); return skillAction("toggle"); }
  if (K.mode === "view") {
    if (cmd && e.key === "Enter") { e.preventDefault(); skillAction("edit"); }
    return;
  }
  if (arrowKey(e, K)) { K.mode = "list"; return drawSkills(); }
  if (e.key === "Tab") { e.preventDefault(); return skillAction(K.mode === "actions" ? "list" : "actions"); }
  if (e.key === "Enter") { e.preventDefault(); return skillAction("view"); }
}
