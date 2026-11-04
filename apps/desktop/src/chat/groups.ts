import type { ApprovalField } from "./chat.ts";
import { grow } from "./chat.ts";
import { copy, renderChats } from "./chats.ts";
import { $, $$, bar, changed, esc, G, GROUP_VIEWS, h, ICON, invoke, S } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
import { appTile, fillAppTiles } from "./applogo.ts";
import { fillPics, picRow } from "./media.ts";
import { dayDiff } from "../lists/memories.ts";

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
interface GroupReminder {
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
/// Beep's avatar is the chat mark the rest of the app uses, never a letter.
const BEEP_MARK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>';
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

function groupHead() {
  const g = G.chat?.group;
  const members = g ? youFirst(g.members).slice(0, 3) : [];
  return `<div class="dhead" id="ghead"><span class="davs">${members.map(m => av(m.you ? "You" : m.name, m.name)).join("")}${av("Beep")}</span>
    <button class="tt" id="gtitle"><b>${esc(g?.name ?? "Group")}</b><small>${g ? esc(people(g)) : "Loading…"}</small></button>
    <button class="dbtn" id="ginvite">Invite</button><button class="dbtn" id="ginfo">Info<kbd>⌘I</kbd></button></div>`;
}
function wireHead() {
  for (const id of ["#gtitle", "#ginfo"]) $(id)?.addEventListener("click", () => renderGroupInfo());
  $("#ginvite")?.addEventListener("click", () => renderInvite(G.id!, G.chat?.group.name));
}
function redrawHead() {
  const old = $("#ghead");
  if (!old) return;
  old.replaceWith(h(groupHead()));
  wireHead();
}

function groupHints() {
  if (G.mention) return hints([["↑↓", "pick"], [ICON.ret, "insert"], ["esc", "close"]]);
  return hints([[ICON.ret, "send"], ["⌘I", "info"], ["⌘K", "chats"], ["esc", "to hide"]]);
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
export async function renderGroup(id: string, then?: "info" | "leave") {
  // The view may stay "group", so a new group is a change of its own.
  if (setGroup(id)) changed();
  G.mention = null;
  G.leave = false;
  S.view = "group";
  const name = G.chat?.group.name ?? "the group";
  bar.innerHTML = `${groupHead()}<div class="dbody pt dgroupbody" id="gbody"></div><div id="mention"></div>
    <div class="dinput small"><textarea id="gask" rows="1" placeholder="Message ${esc(name)}, @beep to ask Beep" autocomplete="off" spellcheck="true"></textarea></div>${groupHints()}`;
  wireHead();
  const ask = $$<HTMLTextAreaElement>("#gask");
  ask.value = G.drafts.get(id) ?? "";
  grow(ask);
  ask.addEventListener("input", () => { G.drafts.set(G.id, ask.value); grow(ask); findMention(); });
  ask.addEventListener("click", findMention);
  if (G.chat) drawGroup(true);
  if (then) {
    await loadGroup();
    if (G.id === id) { G.leave = then === "leave"; renderGroupInfo(G.leave); }
    return;
  }
  ask.focus();
  loadGroup();
}

export async function loadGroup() {
  const id = G.id;
  clearTimeout(G.poll);
  try {
    const chat = await invoke<GroupChat>("open_group", { id });
    if (G.id !== id) return;
    G.chat = chat;
    if (S.view === "group") { redrawHead(); drawGroup(); $$<HTMLTextAreaElement>("#gask").placeholder = `Message ${chat.group.name}, @beep to ask Beep`; }
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
  const b = $("#gbody");
  if (!b || !G.chat) {
    if (b && G.error) b.innerHTML = `<div class="derr">${esc(G.error)}</div>`;
    return;
  }
  const near = jump || b.scrollHeight - b.scrollTop - b.clientHeight < 40;
  const msgs = G.chat.messages;
  let html = "", day = "";
  for (const m of msgs) {
    const d = dayLabel(m.timestamp);
    if (d !== day) { html += `<div class="dday">${esc(d)}</div>`; day = d; }
    html += groupMessage(m);
  }
  for (const p of G.pending) html += `<div class="dg me pending"><div class="b">${mentions(p.text)}</div></div>`;
  if (!msgs.length && !G.pending.length) html += '<div class="dmuted dblock dnomsg">No messages yet.\nSay hi. Beep joins in when it can help.</div>';
  if (G.chat.group.beep_typing) html += '<div class="dmuted"><span class="dtyping"><i></i><i></i><i></i></span>Beep is typing</div>';
  if (G.error) html += `<div class="derr">${esc(G.error)}</div>`;
  b.innerHTML = html;
  fillPics(b);
  for (const a of G.approvals.get(G.id!)?.values() ?? []) b.append(groupApproval(a));
  if (near) b.scrollTop = b.scrollHeight;
}

function groupMessage(m: GroupMessage) {
  const a = m.author;
  const pics = picRow(m.images);
  // A message of pictures alone shows no empty bubble.
  const bubble = (content: string) => m.text || !pics ? `<div class="b">${content}</div>` : "";
  if (a.kind === "member" && a.you) return `<div class="dg me">${bubble(mentions(m.text))}${pics}</div>`;
  const who = authorName(a);
  const content = a.kind === "beep" ? m.html : mentions(m.text);
  return `<div class="dg${a.kind === "beep" ? " beep" : ""}"><span class="who" style="color:${avColor(who)}">${esc(who)}</span>${bubble(content)}${pics}</div>`;
}

/// Beep asks the member whose apps it would use. The card waits on the group.
function groupApproval(a: GroupApproval) {
  const card = h(`<div class="dcard">
    <div class="head">${appTile(a.app)}<div class="dcol"><b>${esc(a.action)}?</b><small>${esc(a.app)} asks you first</small></div></div>
    ${a.fields.length ? `<div class="kv">${a.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-allow="1">Allow</button><button class="dbtn warn" data-allow="">Deny</button></div></div>`);
  fillAppTiles(card);
  card.querySelectorAll<HTMLElement>("[data-allow]").forEach(btn => btn.addEventListener("click", async () => {
    const allow = !!btn.dataset.allow;
    G.approvals.get(a.group_id)?.delete(a.query_id);
    card.replaceWith(h(`<div class="dmuted">${allow ? "Allowed" : "Denied"}</div>`));
    try { await invoke("answer_group_approval", { queryId: a.query_id, allow }); } catch (e) { G.error = String(e); drawGroup(); }
  }));
  return card;
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
export function drawMention() {
  const box = $("#mention");
  if (!box) return;
  const mn = G.mention;
  box.innerHTML = mn ? `<div class="dmlist" role="listbox" aria-label="Mention">${mn.items.map((x, i) =>
    `<button role="option" aria-selected="${i === mn.sel}" data-m="${i}">${av(x.name)}${esc(x.name)}<small>${esc(x.note)}</small></button>`).join("")}</div>` : "";
  box.querySelectorAll<HTMLElement>("[data-m]").forEach(b => b.addEventListener("mousedown", e => { e.preventDefault(); insertMention(+b.dataset.m!); }));
  const old = $(".dhints");
  if (old) old.replaceWith(h(groupHints()));
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

export function renderGroupInfo(leave = false) {
  if (!G.id) return;
  S.view = "ginfo";
  G.leave = leave;
  G.mention = null;
  bar.innerHTML = `${groupHead()}<div class="dbody pt dgroupbody" id="ginfobody"></div>${hints([["esc", "back to the chat"], ["⌘K", "chats"]])}`;
  wireHead();
  G.reminders = null;
  drawGroupInfo();
  const id = G.id;
  invoke<GroupReminder[]>("list_group_reminders", { groupId: id })
    .then(list => { if (G.id === id) { G.reminders = list; drawGroupInfo(); } })
    .catch(() => { if (G.id === id) { G.reminders = []; drawGroupInfo(); } });
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

function groupReminders(g: GroupSummary) {
  const ask = 'Ask Beep in the chat to add one, like "remind us at 10 to check out".';
  const list = G.reminders;
  if (!list) return '<div class="dmuted"><span class="dspin"></span>Loading…</div>';
  if (!list.length) return `<div class="dinfo dmut">No reminders yet. ${esc(ask)}</div>`;
  const you = g.members.find(m => m.you)?.name;
  return `<div class="dinfo">${list.map(r => `<div class="member dalignt">
    <span class="dcol"><b>${esc(r.label)}</b><small>${esc(r.message)}</small><small>${esc(describeSchedule(r.schedule))} · set by ${esc(r.set_by === you ? "you" : r.set_by)}</small></span>
    <button class="dbtn" data-rem="${esc(r.id)}" aria-label="Delete reminder">Delete</button></div>`).join("")}
    <div class="dfine">${esc(ask)}</div></div>`;
}

function drawGroupInfo() {
  const b = $("#ginfobody"), g = G.chat?.group;
  if (!b) return;
  if (!g) { b.innerHTML = '<div class="dmuted"><span class="dspin"></span>Loading…</div>'; return; }
  const date = (ms: number) => new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  const member = (name: string, label: string, note: string) => `<div class="member">${av(name, label)}<span><b>${esc(name)}</b><small>${esc(note)}</small></span></div>`;
  const members = youFirst(g.members).map(m => m.you
    ? member("You", m.name, `${m.name}, ${m.creator ? "created the group" : `joined ${date(m.joined_at)}`}`)
    : member(m.name, m.name, m.creator ? "Created the group" : `Joined ${date(m.joined_at)}`)).join("");
  b.innerHTML = `<div class="dsec flat">Members</div>
    <div class="dinfo">${members}${member("Beep", "Beep", "Joins in when it can help, or when someone writes @beep")}</div>
    <div class="dsec flat">Reminders</div>
    ${groupReminders(g)}
    <div class="dsec flat">Your apps</div>
    <div class="dtoggle"><span><b>Share my apps with Beep here</b><small>${g.apps_shared
      ? "Beep reads them to plan for the group and never shows them here. Sending anything asks you first."
      : "Only when you ask"}</small></span>
      <button class="dswitch" id="gapps" role="switch" aria-checked="${g.apps_shared}" aria-label="Share my apps with Beep here"></button></div>
    <div class="dsec flat">Privacy</div>
    <div class="dinfo dmut"><div>Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.</div><div>Beep's thinking and tool calls are hidden in groups. Members only see its replies.</div></div>
    ${G.leave ? `<div class="dcard"><div class="dask dline wrap"><span class="dfill">Leave ${esc(g.name)}? You stop getting its messages, and need a new code to come back.</span><button class="dbtn" id="gstay">Cancel</button><button class="dbtn warn" id="gleave">Leave</button></div></div>`
      : '<div><button class="dbtn warn" id="gaskleave">Leave group</button></div>'}`;
  // A refusal shows above the info, which stays as it was.
  const fail = (e: unknown) => b.insertAdjacentHTML("afterbegin", `<div class="derr">${esc(e)}</div>`);
  $$("#gapps").addEventListener("click", async () => {
    const shared = !g.apps_shared;
    try { await invoke("share_group_apps", { id: g.id, shared }); g.apps_shared = shared; } catch (e) { fail(e); return; }
    drawGroupInfo();
  });
  b.querySelectorAll<HTMLElement>("[data-rem]").forEach(btn => btn.addEventListener("click", async () => {
    try { await invoke("delete_group_reminder", { groupId: g.id, id: btn.dataset.rem }); } catch (e) { fail(e); return; }
    G.reminders = (G.reminders || []).filter(r => r.id !== btn.dataset.rem);
    drawGroupInfo();
  }));
  $("#gaskleave")?.addEventListener("click", () => { G.leave = true; drawGroupInfo(); });
  $("#gstay")?.addEventListener("click", () => { G.leave = false; drawGroupInfo(); });
  $("#gleave")?.addEventListener("click", async () => {
    try { await invoke("leave_group", { id: g.id }); } catch (e) { fail(e); return; }
    G.approvals.delete(g.id);
    G.drafts.delete(g.id);
    G.id = null;
    G.chat = null;
    S.cache = S.cache.filter(c => c.id !== g.id);
    renderChats();
  });
}

// ---------- Invite ----------

/// A group's invite: the code to read out, its link and QR (InviteView).
interface GroupInvite {
  code: string;
  url: string;
  qr_svg: string;
}

export async function renderInvite(id: string, name?: string | null, reset = false) {
  S.view = "invite";
  setGroup(id);
  const title = name ?? G.chat?.group.name ?? "the group";
  bar.innerHTML = `<div class="dwin"><h4>Invite people</h4><p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p><div class="dmuted"><span class="dspin"></span>Making a code…</div></div>`;
  try {
    const link = await invoke<GroupInvite>("group_invite", { id, reset });
    if (S.view !== "invite" || G.id !== id) return;
    bar.innerHTML = `<div class="dwin"><h4>Invite to ${esc(title)}</h4><p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p>
      <div class="qr">${link.qr_svg}</div><div class="code big">${esc(link.code)}</div>
      <div class="dline wrap dcenter"><button class="dbtn pri" id="icode">Copy code</button><button class="dbtn" id="ilink">Copy link</button><button class="dbtn warn" id="ireset">Reset code</button></div>
      <p class="dfine">${reset ? "New code made. The old one no longer works. " : ""}Everyone who joins sees the whole chat. Beep never shows one person's memories to the others.</p></div>`;
    $$("#icode").addEventListener("click", e => copy(link.code, e.currentTarget as HTMLElement));
    $$("#ilink").addEventListener("click", e => copy(link.url, e.currentTarget as HTMLElement));
    $$("#ireset").addEventListener("click", () => renderInvite(id, title, true));
  } catch (e) {
    if (S.view === "invite") bar.innerHTML = `<div class="dwin"><h4>Invite people</h4><p class="derr">${esc(e)}</p></div>`;
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

export function renderGroupNew(mode: GroupFormMode = "new") {
  S.view = "gnew";
  if (mode !== J.mode) J.error = "";
  J.mode = mode;
  const head = (t: string, sub: string, icon: string) => `<div class="dhead">${icon}<span class="tt"><b>${esc(t)}</b><small>${esc(sub)}</small></span></div>`;
  const logo = `<span class="dlogo" style="background:var(--accent)">${ICON.group}</span>`;
  // Each form's Cancel and the field that takes focus.
  let cancel: () => unknown = renderChats, first: string;
  if (mode === "new") {
    bar.innerHTML = head("New group", "Chat with people, and Beep helps when it can", logo) + `<div class="dform">
      <label class="dfield">Group name<input id="jname" maxlength="80" autocomplete="off" spellcheck="false" value="${esc(J.name ?? "")}"></label>
      <label class="dfield">Your name in the group<input id="jme" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(memberName())}"></label>
      <div class="dmuted dblock">You get a code to send to people. Anyone with Beep who enters it can join.</div>
      ${J.error ? `<div class="derr">${esc(J.error)}</div>` : ""}
      <div class="row"><button class="dbtn" id="jcode">Join with a code</button><span class="dfill"></span><button class="dbtn" id="jcancel">Cancel</button><button class="dbtn pri" id="jgo">Create<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "create"], ["tab", "next field"], ["esc", "back"]]);
    $$("#jcode").addEventListener("click", () => renderGroupNew("code"));
    first = "#jname";
  } else if (mode === "code") {
    bar.innerHTML = head("Join a group", "Enter the code someone sent you", logo) + `<div class="dform">
      <label class="dfield${J.error ? " err" : ""}">Invite code<input id="jtoken" class="mono" maxlength="80" autocomplete="off" spellcheck="false" placeholder="K7QM4-X9TRP">
        <span class="note">${esc(J.error || "Ten letters and digits, like K7QM4-X9TRP. A beep://group link works too.")}</span></label>
      <div class="row"><button class="dbtn" id="jcancel">Cancel</button><button class="dbtn pri" id="jgo">Find group<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "find"], ["esc", "back"]]);
    cancel = () => renderGroupNew("new");
    first = "#jtoken";
  } else {
    const p = J.found!.preview;
    const others = p.members.length ? `${p.members.join(", ")} and Beep are in it` : "Beep is in it";
    bar.innerHTML = head(`Join ${p.name}?`, others, `<span class="davs">${p.members.slice(0, 3).map(n => av(n)).join("")}${av("Beep")}</span>`) + `<div class="dform">
      <div class="dinfo dmut"><div>Beep can use your memories to help the group. It never shows or quotes them to others.</div><div>When Beep answers you, it runs with your apps and your account, never someone else's.</div><div>You get a notification for each new message.</div></div>
      <label class="dfield${J.error ? " err" : ""}">Your name in the group<input id="jme" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(memberName())}">
        <span class="note">${esc(J.error || "The others see this name next to your messages")}</span></label>
      <div class="row"><button class="dbtn" id="jcancel">Not now</button><button class="dbtn pri" id="jgo">Join group<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "join"], ["esc", "back"]]);
    first = "#jme";
  }
  $$("#jcancel").addEventListener("click", cancel);
  $$("#jgo").addEventListener("click", groupFormGo);
  $$(first).focus();
}

export async function groupFormGo() {
  if (J.busy) return;
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
      J.name = "";
      renderInvite(group.id, group.name);
    } else if (J.mode === "code") {
      const code = val("#jtoken");
      if (!code) return;
      try {
        J.found = await invoke<FoundGroup>("peek_group", { code });
      } catch (e) {
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
        keepMemberName(me);
        renderGroup(group.id);
      } catch (e) {
        J.error = String(e);
        renderGroupNew("join");
      }
    }
  } catch (e) {
    J.error = String(e);
    renderGroupNew(J.mode);
  } finally {
    J.busy = false;
  }
}
