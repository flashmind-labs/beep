import { grow } from "./chat.ts";
import { copy, renderChats } from "./chats.ts";
import { $, bar, changed, esc, G, GROUP_VIEWS, h, ICON, invoke, S } from "./core.ts";
import { hints } from "./hints.ts";
import { fillPics, picRow } from "./media.ts";

// ---------- Groups ----------

/// The group on screen (`G`). Drafts and approvals are kept per group.
export interface GroupState {
  id: string | null;
  chat: GroupChat | null;
  pending: PendingMessage[];
  error: string;
  mention: Mention | null;
  poll: ReturnType<typeof setTimeout> | undefined;
  leave: boolean;
  drafts: Map<string | null, string>;
  approvals: Map<string, Map<string, GroupApproval>>;
}
/// The server refuses longer messages.
export const GROUP_LIMIT = 8192;
export const AV_COLORS = ["#F0C14B", "#F07A6A", "#7AB8F0", "#C99BF0", "#F0A35E", "#8FD18B"];

export function authorName(a) { return a.kind === "beep" ? "Beep" : a.you ? "You" : a.name; }
export function avColor(name) {
  if (name === "Beep") return "var(--emerald)";
  let n = 0;
  for (const c of name) n = (n * 31 + c.codePointAt(0)) >>> 0;
  return AV_COLORS[n % AV_COLORS.length];
}
export function av(name, label = name) {
  return `<span class="dav" style="background:${avColor(name)}">${esc(name === "Beep" ? "b" : [...label][0]?.toUpperCase() ?? "?")}</span>`;
}
/// "You, Sam and Beep".
export function people(group) {
  const names = [...group.members].sort((a, b) => b.you - a.you).map(m => m.you ? "You" : m.name);
  names.push("Beep");
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names.at(-1)}` : names[0];
}
export function dayLabel(ms) {
  const d = new Date(ms), now = new Date();
  const start = x => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((start(now) - start(d)) / 86400000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString(undefined, days < 7 ? { weekday: "long" } : { day: "numeric", month: "short" });
}
export function mentions(text) { return esc(text).replace(/@beep\b/gi, m => `<span class="dmention">${m}</span>`); }

export function groupHead() {
  const g = G.chat?.group;
  const members = g ? [...g.members].sort((a, b) => b.you - a.you).slice(0, 3) : [];
  return `<div class="dhead" id="ghead"><span class="davs">${members.map(m => av(m.you ? "You" : m.name, m.name)).join("")}${av("Beep")}</span>
    <button class="tt" id="gtitle"><b>${esc(g?.name ?? "Group")}</b><small>${g ? esc(people(g)) : "Loading…"}</small></button>
    <button class="dbtn" id="ginvite">Invite</button><button class="dbtn" id="ginfo">Info<kbd>⌘I</kbd></button></div>`;
}
export function wireHead() {
  $("#gtitle")?.addEventListener("click", renderGroupInfo);
  $("#ginfo")?.addEventListener("click", renderGroupInfo);
  $("#ginvite")?.addEventListener("click", () => renderInvite(G.id, G.chat?.group.name));
}
export function redrawHead() {
  const old = $("#ghead");
  if (!old) return;
  old.replaceWith(h(groupHead()));
  wireHead();
}

export function groupHints() {
  if (G.mention) return hints([["↑↓", "pick"], [ICON.ret, "insert"], ["esc", "close"]]);
  return hints([[ICON.ret, "send"], ["⌘I", "info"], ["⌘K", "chats"], ["esc", "to hide"]]);
}

/// Opens a group in the bar. `then` opens its info, or info with Leave asked.
export async function renderGroup(id, then) {
  if (G.id !== id) { G.chat = null; G.pending = []; G.error = ""; }
  // The view may stay "group", so a new group is a change of its own.
  if (G.id !== id) { G.id = id; changed(); }
  G.mention = null;
  G.leave = false;
  S.view = "group";
  const name = G.chat?.group.name ?? "the group";
  bar.innerHTML = `${groupHead()}<div class="dbody" id="gbody" style="padding-left:18px;padding-top:12px"></div><div id="mention"></div>
    <div class="dinput small"><textarea id="gask" rows="1" placeholder="Message ${esc(name)}, @beep to ask Beep" autocomplete="off" spellcheck="true"></textarea></div>${groupHints()}`;
  wireHead();
  const ask = $("#gask");
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
    const chat = await invoke("open_group", { id });
    if (G.id !== id) return;
    G.chat = chat;
    if (S.view === "group") { redrawHead(); drawGroup(); $("#gask").placeholder = `Message ${chat.group.name}, @beep to ask Beep`; }
    else if (S.view === "ginfo") drawGroupInfo();
    // The local copy does not know whether Beep is typing, so ask the server.
    if (!chat.group.beep_typing) {
      const typing = await invoke("group_typing", { groupId: id }).catch(() => false);
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

export function drawGroup(jump) {
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
  if (!msgs.length && !G.pending.length) html += '<div class="dmuted" style="display:block;text-align:center;white-space:pre-line;padding:12px 0">No messages yet.\nSay hi. Beep joins in when it can help.</div>';
  if (G.chat.group.beep_typing) html += '<div class="dmuted"><span class="dtyping"><i></i><i></i><i></i></span>Beep is typing</div>';
  if (G.error) html += `<div class="derr">${esc(G.error)}</div>`;
  b.innerHTML = html;
  fillPics(b);
  for (const a of G.approvals.get(G.id)?.values() ?? []) b.append(groupApproval(a));
  if (near) b.scrollTop = b.scrollHeight;
}

export function groupMessage(m) {
  const a = m.author;
  const pics = picRow(m.images);
  // A message of pictures alone shows no empty bubble.
  const bubble = content => m.text || !pics ? `<div class="b">${content}</div>` : "";
  if (a.kind === "member" && a.you) return `<div class="dg me">${bubble(mentions(m.text))}${pics}</div>`;
  const who = authorName(a);
  const content = a.kind === "beep" ? m.html : mentions(m.text);
  return `<div class="dg${a.kind === "beep" ? " beep" : ""}"><span class="who" style="color:${avColor(who)}">${esc(who)}</span>${bubble(content)}${pics}</div>`;
}

/// Beep asks the member whose apps it would use. The card waits on the group.
export function groupApproval(a) {
  const card = h(`<div class="dcard">
    <div class="head"><span class="dapp">${esc((a.app || "?")[0].toUpperCase())}</span><div style="flex:1;min-width:0;display:grid"><b>${esc(a.action)}?</b><small>${esc(a.app)} asks you first</small></div></div>
    ${a.fields.length ? `<div class="kv">${a.fields.map(f => `<span>${esc(f.label)}</span><span>${esc(f.value)}</span>`).join("")}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-allow="1">Allow</button><button class="dbtn warn" data-allow="">Deny</button></div></div>`);
  card.querySelectorAll("[data-allow]").forEach(btn => btn.addEventListener("click", async () => {
    const allow = !!btn.dataset.allow;
    G.approvals.get(a.group_id)?.delete(a.query_id);
    card.replaceWith(h(`<div class="dmuted">${allow ? "Allowed" : "Denied"}</div>`));
    try { await invoke("answer_group_approval", { queryId: a.query_id, allow }); } catch (e) { G.error = String(e); drawGroup(); }
  }));
  return card;
}

export async function sendGroup() {
  const ask = $("#gask"), text = ask.value.trim();
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
    const sent = await invoke("send_to_group", { groupId: id, message: text, images: [] });
    if (G.id !== id) return;
    G.pending = G.pending.filter(x => x !== p);
    if (!G.chat.messages.some(m => m.seq === sent.seq)) G.chat.messages.push(sent);
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
  const ask = $("#gask");
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
  box.querySelectorAll("[data-m]").forEach(b => b.addEventListener("mousedown", e => { e.preventDefault(); insertMention(+b.dataset.m); }));
  const old = $(".dhints");
  if (old) old.replaceWith(h(groupHints()));
}
export function closeMention() {
  if (!G.mention) return;
  G.mention = null;
  drawMention();
}
export function insertMention(i) {
  const ask = $("#gask"), mn = G.mention;
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
  G.leave = leave === true;
  G.mention = null;
  bar.innerHTML = `${groupHead()}<div class="dbody" id="ginfobody" style="padding-left:18px;padding-top:12px;gap:14px"></div>${hints([["esc", "back to the chat"], ["⌘K", "chats"]])}`;
  wireHead();
  G.reminders = null;
  drawGroupInfo();
  const id = G.id;
  invoke("list_group_reminders", { groupId: id })
    .then(list => { if (G.id === id) { G.reminders = list; drawGroupInfo(); } })
    .catch(() => { if (G.id === id) { G.reminders = []; drawGroupInfo(); } });
}

// "Every day at 9:00" for the cron shapes Beep writes, "Tomorrow 10:00" for a one-off.
export function describeSchedule(schedule) {
  const clock = (h, m) => new Date(2000, 0, 1, h, m).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const at = new Date(schedule);
  if (/^\d{4}-\d\d-\d\dT/.test(schedule) && !isNaN(at)) {
    const days = Math.round((new Date(at).setHours(0, 0, 0, 0) - new Date().setHours(0, 0, 0, 0)) / 864e5);
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
  if (dow.split(",").every(d => /^[0-6]$/.test(d))) return `${dow.split(",").map(d => names[d]).join(", ")} at ${clock(h, m)}`;
  return schedule;
}

export function groupReminders(g) {
  const ask = 'Ask Beep in the chat to add one, like "remind us at 10 to check out".';
  const list = G.reminders;
  if (!list) return '<div class="dmuted"><span class="dspin"></span>Loading…</div>';
  if (!list.length) return `<div class="dinfo" style="color:var(--muted)">No reminders yet. ${esc(ask)}</div>`;
  const you = g.members.find(m => m.you)?.name;
  return `<div class="dinfo">${list.map(r => `<div class="member" style="align-items:flex-start">
    <span style="flex:1;min-width:0"><b style="font-weight:500">${esc(r.label)}</b><small>${esc(r.message)}</small><small>${esc(describeSchedule(r.schedule))} · set by ${esc(r.set_by === you ? "you" : r.set_by)}</small></span>
    <button class="dbtn" data-rem="${esc(r.id)}" aria-label="Delete reminder">Delete</button></div>`).join("")}
    <div style="font-size:12px;color:var(--muted)">${esc(ask)}</div></div>`;
}

export function drawGroupInfo() {
  const b = $("#ginfobody"), g = G.chat?.group;
  if (!b) return;
  if (!g) { b.innerHTML = '<div class="dmuted"><span class="dspin"></span>Loading…</div>'; return; }
  const date = ms => new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short" });
  const member = (name, label, note) => `<div class="member">${av(name, label)}<span><b style="font-weight:500">${esc(name)}</b><small>${esc(note)}</small></span></div>`;
  const members = [...g.members].sort((a, b) => b.you - a.you).map(m => m.you
    ? member("You", m.name, `${m.name}, ${m.creator ? "created the group" : `joined ${date(m.joined_at)}`}`)
    : member(m.name, m.name, m.creator ? "Created the group" : `Joined ${date(m.joined_at)}`)).join("");
  b.innerHTML = `<div class="dsec flat">Members</div>
    <div class="dinfo">${members}${member("Beep", "Beep", "Joins in when it can help, or when someone writes @beep")}</div>
    <div class="dsec flat">Reminders</div>
    ${groupReminders(g)}
    <div class="dsec flat">Your apps</div>
    <div class="dtoggle"><span><b>Share my apps with Beep here</b><small>${g.apps_shared
      ? "Beep can read your calendar, mail, files and tasks to plan for the group, and never shows them here. Creating or sending anything asks you first."
      : "Beep uses your apps here only when you ask it yourself."}</small></span>
      <button class="dswitch" id="gapps" role="switch" aria-checked="${g.apps_shared}" aria-label="Share my apps with Beep here"></button></div>
    <div class="dsec flat">Privacy</div>
    <div class="dinfo" style="color:var(--muted)"><div>Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.</div><div>Beep's thinking and tool calls are hidden in groups. Members only see its replies.</div></div>
    ${G.leave ? `<div class="dcard"><div style="padding:10px 12px;display:flex;gap:8px;align-items:center;flex-wrap:wrap"><span style="flex:1;font-size:13px">Leave ${esc(g.name)}? You stop getting its messages, and need a new code to come back.</span><button class="dbtn" id="gstay">Cancel</button><button class="dbtn warn" id="gleave">Leave</button></div></div>`
      : '<div><button class="dbtn warn" id="gaskleave">Leave group</button></div>'}`;
  $("#gapps").addEventListener("click", async () => {
    const shared = !g.apps_shared;
    try { await invoke("share_group_apps", { id: g.id, shared }); g.apps_shared = shared; } catch (e) { b.insertAdjacentHTML("afterbegin", `<div class="derr">${esc(e)}</div>`); return; }
    drawGroupInfo();
  });
  b.querySelectorAll("[data-rem]").forEach(btn => btn.addEventListener("click", async () => {
    try { await invoke("delete_group_reminder", { groupId: g.id, id: btn.dataset.rem }); } catch (e) { b.insertAdjacentHTML("afterbegin", `<div class="derr">${esc(e)}</div>`); return; }
    G.reminders = (G.reminders || []).filter(r => r.id !== btn.dataset.rem);
    drawGroupInfo();
  }));
  $("#gaskleave")?.addEventListener("click", () => { G.leave = true; drawGroupInfo(); });
  $("#gstay")?.addEventListener("click", () => { G.leave = false; drawGroupInfo(); });
  $("#gleave")?.addEventListener("click", async () => {
    try {
      await invoke("leave_group", { id: g.id });
    } catch (e) { b.insertAdjacentHTML("afterbegin", `<div class="derr">${esc(e)}</div>`); return; }
    G.approvals.delete(g.id);
    G.drafts.delete(g.id);
    G.id = null;
    G.chat = null;
    S.cache = S.cache.filter(c => c.id !== g.id);
    renderChats();
  });
}

// ---------- Invite ----------

export async function renderInvite(id, name, reset = false) {
  S.view = "invite";
  if (G.id !== id) { G.chat = null; G.pending = []; G.error = ""; }
  G.id = id;
  invite.name = name ?? G.chat?.group.name ?? "the group";
  bar.innerHTML = `<div class="dwin"><h4>Invite people</h4><p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p><div class="dmuted"><span class="dspin"></span>Making a code…</div></div>`;
  try {
    const link = await invoke("group_invite", { id, reset });
    if (S.view !== "invite" || G.id !== id) return;
    bar.innerHTML = `<div class="dwin"><h4>Invite to ${esc(invite.name)}</h4><p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p>
      <div class="qr">${link.qr_svg}</div><div class="code" style="font-size:22px;font-weight:600">${esc(link.code)}</div>
      <div style="display:flex;gap:8px;flex-wrap:wrap;justify-content:center"><button class="dbtn pri" id="icode">Copy code</button><button class="dbtn" id="ilink">Copy link</button><button class="dbtn warn" id="ireset">Reset code</button></div>
      <p style="font-size:12px">${reset ? "New code made. The old one no longer works. " : ""}Everyone who joins sees the whole chat. Beep never shows one person's memories to the others.</p></div>`;
    $("#icode").addEventListener("click", e => copy(link.code, e.currentTarget));
    $("#ilink").addEventListener("click", e => copy(link.url, e.currentTarget));
    $("#ireset").addEventListener("click", () => renderInvite(id, invite.name, true));
  } catch (e) {
    if (S.view === "invite") bar.innerHTML = `<div class="dwin"><h4>Invite people</h4><p class="derr">${esc(e)}</p></div>`;
  }
}
export const invite = { name: "" };

// ---------- New or join a group ----------

export const MEMBER_NAME = "beep.memberName";
export function memberName() { try { return localStorage.getItem(MEMBER_NAME) ?? ""; } catch { return ""; } }
export function keepMemberName(name) { try { localStorage.setItem(MEMBER_NAME, name); } catch {} }

/// The form under ⌘G: a new group, a code to join one, or the group a code found.
export const J = { mode: "new", name: "", found: null, error: "", busy: false };

export function renderGroupNew(mode = "new") {
  S.view = "gnew";
  if (mode !== J.mode) J.error = "";
  J.mode = mode;
  const head = (t, sub, icon) => `<div class="dhead">${icon}<span class="tt"><b>${esc(t)}</b><small>${esc(sub)}</small></span></div>`;
  const logo = `<span class="dlogo" style="background:var(--emerald)">${ICON.group}</span>`;
  if (mode === "new") {
    bar.innerHTML = head("New group", "Chat with people, and Beep helps when it can", logo) + `<div class="dform" style="padding-top:12px">
      <label class="dfield">Group name<input id="jname" maxlength="80" autocomplete="off" spellcheck="false" value="${esc(J.name ?? "")}"></label>
      <label class="dfield">Your name in the group<input id="jme" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(memberName())}"></label>
      <div class="dmuted" style="display:block">You get a code to send to people. Anyone with Beep who enters it can join.</div>
      ${J.error ? `<div class="derr">${esc(J.error)}</div>` : ""}
      <div class="row"><button class="dbtn" id="jcode">Join with a code</button><span style="flex:1"></span><button class="dbtn" id="jcancel">Cancel</button><button class="dbtn pri" id="jgo">Create<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "create"], ["tab", "next field"], ["esc", "back"]]);
    $("#jcode").addEventListener("click", () => renderGroupNew("code"));
    $("#jcancel").addEventListener("click", renderChats);
    $("#jgo").addEventListener("click", groupFormGo);
    $("#jname").focus();
  } else if (mode === "code") {
    bar.innerHTML = head("Join a group", "Enter the code someone sent you", logo) + `<div class="dform" style="padding-top:12px">
      <label class="dfield${J.error ? " err" : ""}">Invite code<input id="jtoken" class="mono" maxlength="80" autocomplete="off" spellcheck="false" placeholder="K7QM4-X9TRP">
        <span class="note">${esc(J.error || "Ten letters and digits, like K7QM4-X9TRP. A beep://group link works too.")}</span></label>
      <div class="row"><button class="dbtn" id="jcancel">Cancel</button><button class="dbtn pri" id="jgo">Find group<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "find"], ["esc", "back"]]);
    $("#jcancel").addEventListener("click", () => renderGroupNew("new"));
    $("#jgo").addEventListener("click", groupFormGo);
    $("#jtoken").focus();
  } else {
    const p = J.found.preview;
    const others = p.members.length ? `${p.members.join(", ")} and Beep are in it` : "Beep is in it";
    bar.innerHTML = head(`Join ${p.name}?`, others, `<span class="davs">${p.members.slice(0, 3).map(n => av(n)).join("")}${av("Beep")}</span>`) + `<div class="dform" style="padding-top:12px">
      <div class="dinfo" style="color:var(--muted)"><div>Beep can use your memories to help the group. It never shows or quotes them to others.</div><div>When Beep answers you, it runs with your apps and your account, never someone else's.</div><div>You get a notification for each new message.</div></div>
      <label class="dfield${J.error ? " err" : ""}">Your name in the group<input id="jme" maxlength="40" autocomplete="off" spellcheck="false" value="${esc(memberName())}">
        <span class="note">${esc(J.error || "The others see this name next to your messages")}</span></label>
      <div class="row"><button class="dbtn" id="jcancel">Not now</button><button class="dbtn pri" id="jgo">Join group<kbd>${ICON.ret}</kbd></button></div>
    </div>` + hints([[ICON.ret, "join"], ["esc", "back"]]);
    $("#jcancel").addEventListener("click", renderChats);
    $("#jgo").addEventListener("click", groupFormGo);
    $("#jme").focus();
  }
}

export async function groupFormGo() {
  if (J.busy) return;
  const val = sel => $(sel)?.value.trim() ?? "";
  J.busy = true;
  try {
    if (J.mode === "new") {
      const name = val("#jname"), me = val("#jme");
      if (!name) return $("#jname").focus();
      if (!me) return $("#jme").focus();
      keepMemberName(me);
      J.name = name;
      const group = await invoke("create_group", { name, memberName: me });
      J.name = "";
      renderInvite(group.id, group.name);
    } else if (J.mode === "code") {
      const code = val("#jtoken");
      if (!code) return;
      try {
        J.found = await invoke("peek_group", { code });
      } catch (e) {
        J.error = String(e);
        return renderGroupNew("code");
      }
      if (J.found.preview.joined) return renderGroup(J.found.preview.id);
      J.error = "";
      renderGroupNew("join");
    } else {
      const me = val("#jme");
      if (!me) return $("#jme").focus();
      try {
        const group = await invoke("join_group", { token: J.found.token, memberName: me });
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

