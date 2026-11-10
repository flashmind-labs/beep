import { $$, S } from "../bar/core.ts";
import { hintModel } from "../bar/hints.ts";
import { arrowKey, clampSel, confirmKey, type ListState, loadList, readDay } from "./memories.ts";
import { dayDiff, plural } from "../ui/format.ts";
import { ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { confirmSheet, openMenu, type MenuItem } from "../ui/menu.ts";
import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";

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
export type SkillAction = SkillMode | "toggle" | "undo" | "save" | "delete-yes";
const MODES = new Set<string>(["list", "actions", "delete", "view", "edit", "vdelete"]);
/// The modes that show one skill rather than the list.
const ONE = new Set<string>(["view", "edit", "vdelete"]);

export const K: ListState<Skill> & { mode: SkillMode } = { list: [], sel: 0, mode: "list", error: "", loaded: false };
let editor: BarPageSlot | null = null;

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

const skillsRoot = () => editor ??= openBarPage(() => { editor = null; });

export function drawSkills(): void {
  clampSel(K);
  if (ONE.has(K.mode)) return drawSkill();
  const paused = K.list.filter(k => !k.enabled).length;
  const sum = !K.loaded ? "" : !K.list.length ? "None yet" : [plural(K.list.length, "skill", "skills"), paused && `${paused} paused`].filter(Boolean).join(", ");
  const keys = K.loaded && !K.list.length ? [["esc", "back"]] : K.mode === "actions"
    ? [[ICON.ret, "open"], ["⌘Z", "undo"], ["⌘⌫", "delete"], ["tab", "hide actions"], ["esc", "back"]]
    : [["↑↓", "move"], [ICON.ret, "open"], ["space", "on or off"], ["tab", "actions"], ["esc", "back"]];
  const root = skillsRoot();
  flushSync(() => root.show({ view: "skills", props: { list: K.list, selected: K.sel, mode: K.mode as "list" | "actions" | "delete", loaded: K.loaded, error: K.error, summary: sum, hints: hintModel(keys),
    act: skillAct, pick: pickSkill, menu: skillMenu } }));
}

/// One skill: its instructions, or the editor.
export function drawSkill(): void {
  const k = K.list[K.sel];
  if (!k) { K.mode = "list"; return drawSkills(); }
  const root = skillsRoot();
  if (K.mode === "edit") {
    flushSync(() => root.show({ view: "skill-editor", props: { skill: k, error: K.error, hints: hintModel([["tab", "next field"], [`⌘${ICON.ret}`, "save"], ["esc", "cancel"]]), save: () => { void skillAction("save"); }, cancel: () => { void skillAction("view"); } } }));
    return;
  }
  const keys = K.mode === "vdelete" ? [[ICON.ret, "delete"], ["esc", "cancel"]] : [["space", "on or off"], [`⌘${ICON.ret}`, "edit"], ...(k.can_undo ? [["⌘Z", "undo"]] : []), ["esc", "back"]];
  flushSync(() => root.show({ view: "skill", props: { skill: k, mode: K.mode as "view" | "vdelete", error: K.error, hints: hintModel(keys), act: skillAct } }));
}

/// A skill row's right-click menu: what its buttons do. Delete asks first.
function skillMenu(e: MouseEvent, index: number) {
  K.sel = index;
  const k = K.list[K.sel];
  if (!k) return;
  const items: MenuItem[] = [
    { label: "Open", on: () => void skillAction("view") },
    { label: k.enabled ? "Turn off" : "Turn on", on: () => void skillAction("toggle") },
  ];
  if (k.can_undo) items.push({ label: "Undo", on: () => void skillAction("undo") });
  items.push({
    label: "Delete…",
    warn: true,
    on: () => confirmSheet({
      title: `Delete ${k.name}?`,
      text: "The skill is deleted for good.",
      action: "Delete",
      run: () => skillAction("delete-yes"),
    }),
  });
  openMenu(e, k.name, items);
}

/// A button of the list or of the open skill: one on a row acts on that row.
function skillAct(a: SkillAction, index: number | null) {
  if (index !== null) K.sel = index;
  void skillAction(a);
}

/// A click on a row opens the skill, unless a delete is being confirmed.
function pickSkill(index: number) {
  if (K.mode === "delete") return;
  K.sel = index;
  void skillAction("view");
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
