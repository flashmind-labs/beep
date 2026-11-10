import { createElement, Fragment, type MouseEvent, type ReactElement } from "react";
import { flushSync } from "react-dom";
import type { Root } from "react-dom/client";
import { openBarPage } from "../bar/bar-app.tsx";
import { GroupHeader, GroupShell } from "./group-shell-view.tsx";
import { GroupFormView } from "./group-form-view.tsx";
import { GroupInviteView, type GroupInvite, type InviteState } from "./group-invite-view.tsx";
import { GroupInfoView } from "./group-info-view.tsx";
import { GroupMessages } from "./group-view.tsx";
import type { ApprovalField } from "./approval-cards.ts";
import { grow } from "./ask-height.ts";
import { renderChats } from "./chats.ts";
import { copy } from "../ui/parts.ts";
import { $, $$, changed, G, GROUP_VIEWS, S } from "../bar/core.ts";
import { hintModel } from "../bar/hints.ts";
import { HintView } from "../bar/hints-view.tsx";
import { dayDiff, esc } from "../ui/format.ts";
import { BEEP_MARK, ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";

// ---------- Groups ----------

/// A member of a group (GroupMember).
export interface GroupMember {
  name: string;
  /// The caller.
  you: boolean;
  /// Made the group.
  creator: boolean;
  /// In milliseconds since the Unix epoch.
  joined_at: number;
}

/// Who wrote a group message.
export type Author = { kind: "member"; name: string; you: boolean } | { kind: "beep" };

/// A picture a message carried (ImageRef).
export interface GroupImage {
  id: string;
  mime: string;
  width: number;
  height: number;
}

/// A group message (MessageView).
export interface GroupMessage {
  seq: number;
  author: Author;
  /// As written. People's messages show as plain text.
  text: string;
  /// Beep's replies as sanitized HTML, empty for people's.
  html: string;
  timestamp: number;
  images: GroupImage[];
}

/// A group (GroupView), as `list_groups` lists it and a chat opens with it.
export interface GroupSummary {
  id: string;
  name: string;
  members: GroupMember[];
  last: GroupMessage | null;
  updated_at: number;
  beep_typing: boolean;
  pinned: boolean;
  apps_shared: boolean;
}

/// A group with its latest messages, oldest first (GroupChatView).
export interface GroupChat {
  group: GroupSummary;
  messages: GroupMessage[];
}

/// The @ picker open under the composer: where the typed mention starts
/// and ends, who it offers and which is picked.
export interface Mention {
  at: number;
  end: number;
  items: { name: string; note: string }[];
  sel: number;
}

/// A tool call in a group waiting for this user's OK (GroupApproval).
export interface GroupApproval {
  group_id: string;
  query_id: string;
  app: string;
  action: string;
  fields: ApprovalField[];
}

/// A reminder Beep posts into a group when it is due (GroupReminder).
export interface GroupReminder {
  id: string;
  label: string;
  message: string;
  /// RFC 3339 for a one-off, else a cron expression.
  schedule: string;
  next_run_at: string | null;
  /// The member who asked for it, by their name in the group.
  set_by: string;
}

/// The group on screen (`G`). Drafts and approvals are kept per group.
export interface GroupState {
  id: string | null;
  chat: GroupChat | null;
  /// Messages sent from here the server has not confirmed yet.
  pending: { text: string }[];
  error: string;
  mention: Mention | null;
  poll: ReturnType<typeof setTimeout> | undefined;
  leave: boolean;
  drafts: Map<string | null, string>;
  approvals: Map<string, Map<string, GroupApproval>>;
  /// Group info's reminders: null while they load.
  reminders?: GroupReminder[] | null;
}
/// The server refuses longer messages.
const GROUP_LIMIT = 8192;
const AV_COLORS = ["#F0C14B", "#F07A6A", "#7AB8F0", "#C99BF0", "#F0A35E", "#8FD18B"];

export function authorName(a: Author) { return a.kind === "beep" ? "Beep" : a.you ? "You" : a.name; }
function avColor(name: string) {
  if (name === "Beep") return "var(--accent)";
  let n = 0;
  for (const c of name) n = (n * 31 + c.codePointAt(0)!) >>> 0;
  return AV_COLORS[n % AV_COLORS.length];
}
function av(name: string, label = name) {
  return `<span class="dav" style="background:${avColor(name)}">${name === "Beep" ? BEEP_MARK : esc([...label][0]?.toUpperCase() ?? "?")}</span>`;
}
/// The members with the caller first.
const youFirst = (members: GroupMember[]) => [...members].sort((a, b) => Number(b.you) - Number(a.you));
/// "You, Sam and Beep".
function people(group: GroupSummary) {
  const names = youFirst(group.members).map(m => m.you ? "You" : m.name);
  names.push("Beep");
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : names[0];
}
function dayLabel(ms: number) {
  const d = new Date(ms);
  const days = dayDiff(d, new Date());
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, days < 7 ? { weekday: "long" } : { day: "numeric", month: "short" });
}
function mentions(text: string) { return esc(text).replace(/@beep\b/gi, m => `<span class="dmention">${m}</span>`); }

function groupHints() {
  if (G.mention) return hintModel([["↑↓", "pick"], [ICON.ret, "insert"], ["esc", "close"]]);
  return hintModel([[ICON.ret, "send"], ["⌘I", "info"], ["⌘K", "chats"], ["esc", "to hide"]]);
}

/// Makes `id` the group on screen, starting empty when it is another one.
/// True when it changed.
function setGroup(id: string) {
  if (G.id === id) return false;
  G.chat = null;
  G.pending = [];
  G.error = "";
  G.id = id;
  return true;
}

/// Opens a group in the bar. `then` opens its info, or info with Leave asked.
let messageRoot: { host: HTMLElement; root: Root; message: ReactElement | null; mention: ReactElement | null; draft: string } | null = null;
const pendingKeys = new WeakMap<object, number>();
let nextPending = 0, groupRevision = 0;
const shareVersions = new Map<string, number>(), reminderVersions = new Map<string, number>();
function disposeMessages() { messageRoot = null; }

export async function renderGroup(id: string, then?: "info" | "leave") {
  disposeInfo();
  disposeMessages();
  // The view may stay "group", so a new group is a change of its own.
  if (setGroup(id)) changed();
  G.mention = null;
  G.leave = false;
  S.view = "group";
  const { host, root } = openBarPage(() => { messageRoot = null; });
  messageRoot = { host, root, message: null, mention: null, draft: G.drafts.get(id) ?? "" };
  drawGroupRoot(messageRoot);
  if (G.chat) drawGroup(true);

  if (then) {
    await loadGroup();
    if (G.id === id) { G.leave = then === "leave"; renderGroupInfo(G.leave); }
    return;
  }

  $("#gask")?.focus();
  loadGroup();
}

export async function loadGroup() {
  const id = G.id, shareVersion = id ? shareVersions.get(id) ?? 0 : 0;
  clearTimeout(G.poll);

  try {
    const chat = await invoke<GroupChat>("open_group", { id });
    if (G.id !== id) return;
    if (id && (shareVersions.get(id) ?? 0) !== shareVersion && G.chat) chat.group.apps_shared = G.chat.group.apps_shared;
    G.chat = chat;

    if (S.view === "group") { drawGroup(); if (G.mention) findMention(); }
    else if (S.view === "ginfo") drawGroupInfo();

    // The local copy does not know whether Beep is typing, so ask the server.
    if (!chat.group.beep_typing) {
      const typing = await invoke<boolean>("group_typing", { groupId: id }).catch(() => false);
      if (G.chat !== chat || !typing) return;
      chat.group.beep_typing = true;
      if (S.view === "group") drawGroup();
    }

    // No notice comes while Beep types, so look again until it is done.
    if (chat.group.beep_typing) G.poll = setTimeout(() => { if (G.id === id && GROUP_VIEWS.has(S.view)) loadGroup(); }, 5000);
  } catch (e) {
    if (G.id !== id || S.view !== "group") return;
    G.error = String(e);
    drawGroup();
  }
}

export function drawGroup(jump = false) {
  const current = messageRoot, b = $("#gbody");
  if (!current || !b || !current.host.isConnected) return;
  const near = jump || b.scrollHeight - b.scrollTop - b.clientHeight < 40;
  const pending = G.pending.map(item => {
    let key = pendingKeys.get(item);
    if (key === undefined) { key = ++nextPending; pendingKeys.set(item, key); }
    return { key, text: item.text };
  });
  const approvals = G.id ? [...(G.approvals.get(G.id)?.values() ?? [])] : [];
  current.message = createElement(GroupMessages, {
    chat: G.chat, error: G.error, revision: ++groupRevision, pending, approvals,
    day: dayLabel, author: (m: GroupMessage) => authorName(m.author), color: avColor, mentions,
    answer: (approval: GroupApproval, allow: boolean) => {
      G.approvals.get(approval.group_id)?.delete(approval.query_id);
      void invoke("answer_group_approval", { queryId: approval.query_id, allow }).catch(error => { G.error = String(error); drawGroup(); });
    },
  });
  drawGroupRoot(current);
  if (near) b.scrollTop = b.scrollHeight;
}

export async function sendGroup() {
  const ask = $$<HTMLTextAreaElement>("#gask"), text = ask.value.trim();
  if (!text || !G.chat) return;
  if (new TextEncoder().encode(text).length > GROUP_LIMIT) { G.error = "This message is too long to send."; return drawGroup(); }
  const id = G.id, p = { text };
  G.error = "";
  G.pending.push(p);
  G.drafts.delete(id);
  ask.value = "";
  grow(ask);
  closeMention();
  drawGroup(true);

  try {
    const sent = await invoke<GroupMessage>("send_to_group", { groupId: id, message: text, images: [] });
    if (G.id !== id) return;
    G.pending = G.pending.filter(x => x !== p);
    if (!G.chat!.messages.some(m => m.seq === sent.seq)) G.chat!.messages.push(sent);
    if (S.view === "group") drawGroup(true);
    loadGroup();
  } catch (e) {
    if (G.id !== id) return;
    G.pending = G.pending.filter(x => x !== p);
    G.error = `Not sent: ${e}`;

    if (S.view === "group") {
      if (!ask.value) { ask.value = text; G.drafts.set(id, text); grow(ask); }
      drawGroup(true);
    }
  }
}

// @ opens a picker of Beep and the other members, like the phone's.
export function findMention() {
  const ask = $<HTMLTextAreaElement>("#gask");
  if (!ask || !G.chat) return;
  const before = ask.value.slice(0, ask.selectionStart);
  const m = before.match(/(^|\s)@([\p{L}\p{N}_]*)$/u);
  if (!m) return closeMention();
  const typed = m[2].toLowerCase();
  const items = [{ name: "Beep", note: "Make sure Beep replies" },
    ...G.chat.group.members.filter(x => !x.you).map(x => ({ name: x.name, note: "Member" }))]
    .filter(x => x.name.toLowerCase().startsWith(typed));
  if (!items.length) return closeMention();
  const sel = G.mention && G.mention.items.length === items.length ? Math.min(G.mention.sel, items.length - 1) : 0;
  G.mention = { at: before.length - m[2].length - 1, end: before.length, items, sel };
  drawMention();
}
function drawGroupRoot(current: NonNullable<typeof messageRoot>) {
  const group = G.chat?.group;
  const members = group ? youFirst(group.members).slice(0, 3) : [];
  flushSync(() => current.root.render(createElement(GroupShell, {
    name: group?.name ?? "Group", placeholder: group?.name ?? "the group", people: group ? people(group) : "Loading…",
    avatars: members.map(m => av(m.you ? "You" : m.name, m.name)).join("") + av("Beep"), draft: current.draft,
    message: current.message, mention: current.mention, hints: groupHints(), grow,
    input: (ask: HTMLTextAreaElement) => { G.drafts.set(G.id, ask.value); grow(ask); findMention(); },
    find: findMention, info: () => { void renderGroupInfo(); }, invite: () => { if (G.id) void renderInvite(G.id, G.chat?.group.name); },
  })));
}
export function drawMention() {
  const current = messageRoot;
  if (!current || !current.host.isConnected) return;
  const mn = G.mention;
  current.mention = mn ? createElement("div", { className: "dmlist", role: "listbox", "aria-label": "Mention" },
    mn.items.map((item, i) => createElement("button", {
      key: `${item.name}-${i}`, role: "option", "aria-selected": i === mn.sel, "data-m": i,
      onMouseDown: (event: MouseEvent) => { event.preventDefault(); insertMention(i); },
    }, createElement("span", { className: "dav", style: { background: avColor(item.name) }, dangerouslySetInnerHTML: { __html: item.name === "Beep" ? BEEP_MARK : esc([...item.name][0]?.toUpperCase() ?? "?") } }), item.name, createElement("small", null, item.note)))
  ) : null;
  drawGroupRoot(current);
}
export function closeMention() {
  if (!G.mention) return;
  G.mention = null;
  drawMention();
}
export function insertMention(i: number) {
  const ask = $<HTMLTextAreaElement>("#gask"), mn = G.mention;
  if (!ask || !mn) return;
  const word = mn.items[i].name === "Beep" ? "@beep " : `@${mn.items[i].name} `;
  ask.value = ask.value.slice(0, mn.at) + word + ask.value.slice(mn.end);
  const caret = mn.at + word.length;
  ask.setSelectionRange(caret, caret);
  G.drafts.set(G.id, ask.value);
  grow(ask);
  closeMention();
  ask.focus();
}

// ---------- Group info ----------

let infoRoot: { host: HTMLElement; root: Root; id: string; error: string } | null = null;
const infoBusy = new Set<string>();
function disposeInfo() { infoRoot = null; }
export function renderGroupInfo(leave = false) {
  if (!G.id) return;
  disposeMessages(); disposeInfo();
  S.view = "ginfo";
  G.leave = leave; G.mention = null;
  const id = G.id, { host, root } = openBarPage(() => { infoRoot = null; });
  infoRoot = { host, root, id, error: "" };
  const owner = infoRoot;
  G.reminders = null;
  drawGroupInfo();
  loadGroupReminders(owner);
}
function loadGroupReminders(owner: NonNullable<typeof infoRoot>) {
  const id = owner.id, version = reminderVersions.get(id) ?? 0;
  invoke<GroupReminder[]>("list_group_reminders", { groupId: id })
    .then(list => { if (infoRoot === owner && G.id === id && (reminderVersions.get(id) ?? 0) === version) { G.reminders = list; drawGroupInfo(); } })
    .catch(() => { if (infoRoot === owner && G.id === id && (reminderVersions.get(id) ?? 0) === version) { G.reminders = []; drawGroupInfo(); } });
}

// "Every day at 9:00" for the cron shapes Beep writes, "Tomorrow 10:00" for a one-off.
function describeSchedule(schedule: string) {
  const clock = (h: number, m: number) => new Date(2000, 0, 1, h, m).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const at = new Date(schedule);

  if (/^\d{4}-\d\d-\d\dT/.test(schedule) && !isNaN(at.getTime())) {
    const days = dayDiff(new Date(), at);
    const day = days === 0 ? "Today" : days === 1 ? "Tomorrow" : days > 1 && days < 7
      ? at.toLocaleDateString(undefined, { weekday: "short" }) : at.toLocaleDateString(undefined, { day: "numeric", month: "short" });
    return `${day} ${clock(at.getHours(), at.getMinutes())}`;
  }

  const fields = schedule.trim().split(/\s+/);
  if (fields.length !== 5) return schedule;
  const [min, hour, dom, mon, dow] = fields;
  const m = Number(min), h = Number(hour);
  if (dom !== "*" || mon !== "*" || !Number.isInteger(m) || !Number.isInteger(h)) return schedule;
  const names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  if (dow === "*") return `Every day at ${clock(h, m)}`;
  if (dow === "1-5") return `Weekdays at ${clock(h, m)}`;
  if (dow.split(",").every(d => /^[0-6]$/.test(d))) return `${dow.split(",").map(d => names[Number(d)]).join(", ")} at ${clock(h, m)}`;
  return schedule;
}

function drawGroupInfo() {
  const current = infoRoot, group = G.chat?.group ?? null;
  if (!current?.host.isConnected || S.view !== "ginfo" || G.id !== current.id) return;
  const pending = new Set([...infoBusy].filter(key => key.startsWith(`${current.id}\0`)).map(key => key.slice(current.id.length + 1)));
  const members = group ? youFirst(group.members).slice(0, 3) : [];
  const header = createElement(GroupHeader, { name: group?.name ?? "Group", people: group ? people(group) : "Loading…", avatars: members.map(m => av(m.you ? "You" : m.name, m.name)).join("") + av("Beep"), info: () => renderGroupInfo(), invite: () => { if (G.id) void renderInvite(G.id, group?.name); } });
  const content = createElement(GroupInfoView, {
    group, reminders: G.reminders ?? null, leave: G.leave, error: current.error, busy: pending, color: avColor, schedule: describeSchedule,
    share: () => { if (group) void changeGroupInfo(current, "share", async () => {
      const shared = !group.apps_shared;
      await invoke("share_group_apps", { id: group.id, shared });
      shareVersions.set(group.id, (shareVersions.get(group.id) ?? 0) + 1);
      if (G.id === group.id && G.chat?.group.id === group.id) G.chat.group.apps_shared = shared;
    }); },
    remove: (id: string) => { if (group) void changeGroupInfo(current, `reminder:${id}`, async () => {
      await invoke("delete_group_reminder", { groupId: group.id, id });
      reminderVersions.set(group.id, (reminderVersions.get(group.id) ?? 0) + 1);
      if (G.id === group.id) {
        if (G.reminders) G.reminders = G.reminders.filter(reminder => reminder.id !== id);
        else if (infoRoot?.id === group.id) loadGroupReminders(infoRoot);
      }
    }); },
    askLeave: () => { G.leave = true; drawGroupInfo(); }, stay: () => { G.leave = false; drawGroupInfo(); },
    leaveGroup: () => { if (group) void changeGroupInfo(current, "leave", async () => {
      await invoke("leave_group", { id: group.id });
      G.approvals.delete(group.id); G.drafts.delete(group.id); S.cache = S.cache.filter(chat => chat.id !== group.id);
      if (G.id === group.id) { G.id = null; G.chat = null; renderChats(); }
    }); },
  });
  flushSync(() => current.root.render(createElement(Fragment, null, header, createElement("div", { className: "dbody pt dgroupbody", id: "ginfobody" }, content), createElement(HintView, { value: hintModel([["esc", "back to the chat"], ["⌘K", "chats"]]) }))));
}
async function changeGroupInfo(owner: NonNullable<typeof infoRoot>, action: string, run: () => Promise<void>) {
  const key = `${owner.id}\0${action}`;
  if (infoBusy.has(key)) return;
  infoBusy.add(key); owner.error = ""; drawGroupInfo();
  try { await run(); } catch (error) { if (infoRoot === owner) owner.error = String(error); }
  finally { infoBusy.delete(key); if (infoRoot?.id === owner.id) drawGroupInfo(); }
}

// ---------- Invite ----------

let inviteRoot: { host: HTMLElement; root: Root; id: string; title: string; revision: number } | null = null;
function disposeInvite() { inviteRoot = null; }

export async function renderInvite(id: string, name?: string | null, reset = false) {
  disposeInvite();
  S.view = "invite";
  setGroup(id);
  const title = name ?? G.chat?.group.name ?? "the group";
  const { host, root } = openBarPage(() => { inviteRoot = null; });
  const owner = { host, root, id, title, revision: 0 };
  inviteRoot = owner;
  await loadInvite(owner, reset);
}

async function loadInvite(owner: NonNullable<typeof inviteRoot>, reset: boolean) {
  const revision = ++owner.revision;
  const draw = (state: InviteState) => flushSync(() => owner.root.render(createElement(GroupInviteView, {
    title: owner.title, state, copy, reset: () => { void loadInvite(owner, true); },
  })));
  draw({ kind: "loading" });
  try {
    const invite = await invoke<GroupInvite>("group_invite", { id: owner.id, reset });
    if (inviteRoot === owner && owner.revision === revision && S.view === "invite" && G.id === owner.id) draw({ kind: "ready", invite, reset });
  } catch (error) {
    if (inviteRoot === owner && owner.revision === revision && S.view === "invite" && G.id === owner.id) draw({ kind: "error", message: String(error) });
  }
}

// ---------- New or join a group ----------

const MEMBER_NAME = "beep.memberName";
function memberName() { try { return localStorage.getItem(MEMBER_NAME) ?? ""; } catch { return ""; } }
function keepMemberName(name: string) { try { localStorage.setItem(MEMBER_NAME, name); } catch {} }

/// What a typed code found (Found): the group and the token that joins it.
interface FoundGroup {
  token: string;
  preview: {
    id: string;
    name: string;
    /// Member names, oldest first.
    members: string[];
    /// The caller is in it already.
    joined: boolean;
  };
}

/// The form under ⌘G: a new group, a code to join one, or the group a code found.
export type GroupFormMode = "new" | "code" | "join";

/// The ⌘G form's state.
export const J: { mode: GroupFormMode; name: string; found: FoundGroup | null; error: string; busy: boolean } = { mode: "new", name: "", found: null, error: "", busy: false };

let formRoot: { host: HTMLElement; root: Root } | null = null;
function disposeGroupForm() { formRoot = null; }

export function renderGroupNew(mode: GroupFormMode = "new") {
  disposeGroupForm();
  J.busy = false;
  S.view = "gnew";
  if (mode !== J.mode) J.error = "";
  J.mode = mode;
  const { host, root } = openBarPage(() => { formRoot = null; });
  formRoot = { host, root };
  const members = J.found?.preview.members ?? [];
  const help = mode === "new" ? [[ICON.ret, "create"], ["tab", "next field"], ["esc", "back"]] : [[ICON.ret, mode === "code" ? "find" : "join"], ["esc", "back"]];
  flushSync(() => root.render(createElement(GroupFormView, {
    mode, name: J.name, member: memberName(), error: J.error, preview: J.found?.preview ?? null,
    avatars: members.slice(0, 3).map(n => av(n)).join("") + av("Beep"), hints: hintModel(help),
    changeMode: renderGroupNew, cancel: renderChats, go: () => void groupFormGo(),
  })));
}

export async function groupFormGo() {
  const owner = formRoot;
  if (J.busy || !owner) return;
  const current = () => formRoot === owner && S.view === "gnew";
  const val = (sel: string) => $<HTMLInputElement>(sel)?.value.trim() ?? "";
  const focus = (sel: string) => $$(sel).focus();
  J.busy = true;

  try {
    if (J.mode === "new") {
      const name = val("#jname"), me = val("#jme");
      if (!name) return focus("#jname");
      if (!me) return focus("#jme");
      keepMemberName(me);
      J.name = name;
      const group = await invoke<GroupSummary>("create_group", { name, memberName: me });
      if (!current()) return;
      J.name = "";
      renderInvite(group.id, group.name);
    } else if (J.mode === "code") {
      const code = val("#jtoken");
      if (!code) return;

      try {
        const found = await invoke<FoundGroup>("peek_group", { code });
        if (!current()) return;
        J.found = found;
      } catch (e) {
        if (!current()) return;
        J.error = String(e);
        return renderGroupNew("code");
      }

      if (J.found.preview.joined) return renderGroup(J.found.preview.id);
      J.error = "";
      renderGroupNew("join");
    } else {
      const me = val("#jme");
      if (!me) return focus("#jme");

      try {
        const group = await invoke<GroupSummary>("join_group", { token: J.found!.token, memberName: me });
        if (!current()) return;
        keepMemberName(me);
        renderGroup(group.id);
      } catch (e) {
        if (!current()) return;
        J.error = String(e);
        renderGroupNew("join");
      }
    }
  } catch (e) {
    if (!current()) return;
    J.error = String(e);
    renderGroupNew(J.mode);
  } finally {
    if (current()) J.busy = false;
  }
}
