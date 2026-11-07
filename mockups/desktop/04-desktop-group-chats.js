// ---- Desktop group chats ----
// The bar shows a group the same way it shows a chat: a header with the people, the messages, and a field at the bottom.
// Beep's thinking and tools stay hidden in groups, so only its replies show.
const DM = { You: "#F0C14B", Sam: "#F2994A", Beep: "#5E6AD2" };
const dAv = (n, size = "") => `<span class="dav" style="background:${DM[n]};${size}">${n === "Beep" ? DI.tray : n[0]}</span>`;
const dGroupHead = (extra = "") => `<div class="dhead"><span class="davs">${["You", "Sam", "Beep"].map(n => dAv(n)).join("")}</span>
  <button class="tt" style="border:0;background:transparent;color:inherit;text-align:left;padding:0" data-dgo="dGroupInfo"><b>Tokyo trip</b><small>You, Sam and Beep</small></button>
  ${extra}<button class="dbtn" data-dgo="dGroupInvite">Invite</button><button class="dbtn" data-dgo="dGroupInfo">Info<kbd>⌘I</kbd></button></div>`;
const dgMsg = (who, html, cls = "") => who === "You"
  ? `<div class="dg me ${cls}"><div class="b">${html}</div></div>`
  : `<div class="dg${who === "Beep" ? " beep" : ""} ${cls}"><span class="who" style="color:${DM[who]}">${who}</span><div class="b">${html}</div></div>`;
const at = '<span class="dmention">@beep</span>';
Object.assign(D, {
  dGroup: () => {
    const st = S.d.group;
    const reply = `Two quiet ones near Asakusa, both with private baths and under ¥40,000 a night:<ul style="margin:4px 0"><li><b>Sadachiyo</b>: old wooden inn, kaiseki dinner with a vegetarian set</li><li><b>Kamogawa Asakusa</b>: five minutes from Senso-ji, breakfast from 6:30</li></ul>Both have rooms free on those nights. Want me to hold one?`;
    const tail = st === "typing" ? `<div class="dmuted"><span class="dtyping"><i></i><i></i><i></i></span>Beep is typing</div>`
      : st === "sending" ? dgMsg("Beep", reply) + dgMsg("You", `${at} hold Sadachiyo for the three of us`, "pending")
      : dgMsg("Beep", reply);
    const mention = st === "mention";
    const picker = mention ? `<div class="dmlist" role="listbox" aria-label="Mention">
        <button role="option" aria-selected="true">${dAv("Beep")}<b style="font-weight:600">Beep</b><small>Make sure Beep replies</small></button>
        <button role="option" aria-selected="false">${dAv("Sam")}Sam<small>Member</small></button></div>` : "";
    const field = mention
      ? `<div class="dinput small"><span class="txt">@<span class="caret" style="height:18px"></span></span></div>`
      : `<div class="dinput small" style="cursor:text" data-dact="group:mention"><span class="caret" style="height:18px"></span><span class="txt ph" style="margin-left:-8px">Message Tokyo trip, @beep to ask Beep</span></div>`;
    return deskScene(dBar(dGroupHead() + `<div class="dbody" style="padding-left:18px;padding-top:12px">
      <div class="dday">Yesterday</div>
      ${dgMsg("Sam", "Flights are booked! Landing at Haneda on the 14th, 16:20")}
      ${dgMsg("You", "Nice. Three nights in Tokyo then Kyoto?")}
      <div class="dday">Today</div>
      ${dgMsg("Sam", `${at} can you find us a ryokan for the 14th to the 17th? Something quiet`)}
      ${tail}
    </div>` + picker + field + dHints(mention ? [["↑↓", "pick"], [DI.ret, "insert"], ["esc", "close"]] : [[DI.ret, "send"], ["⌘I", "info"], ["⌘K", "chats"], ["esc", "to hide"]])));
  },
  // ⌘G opens this in the bar. The last name used is filled in.
  dGroupNew: () => {
    const st = S.d.gnew;
    const head = (t, sub) => `<div class="dhead"><span class="dlogo" style="background:var(--d-emerald)">${DI.group}</span><span class="tt"><b>${t}</b><small>${sub}</small></span></div>`;
    if (st === "new") return deskScene(dBar(head("New group", "Chat with people, and Beep helps when it can") + `<div class="dform" style="padding-top:12px">
      <label class="dfield on">Group name<span class="f">Tokyo trip<span class="caret" style="height:16px"></span></span></label>
      <label class="dfield">Your name in the group<span class="f">Dani</span></label>
      <div class="dmuted" style="display:block">You get a code to send to people. Anyone with Beep who enters it can join.</div>
      <div style="display:flex;gap:8px;align-items:center"><button class="dbtn" data-dact="gnew:code">Join with a code</button><span style="flex:1"></span><button class="dbtn quiet" data-dgo="dChats">Cancel</button><button class="dbtn pri" data-dgo="dGroupInvite">Create<kbd>${DI.ret}</kbd></button></div>
    </div>` + dHints([[DI.ret, "create"], ["tab", "next field"], ["esc", "back"]])));
    if (st === "code" || st === "wrong") return deskScene(dBar(head("Join a group", "Enter the code someone sent you") + `<div class="dform" style="padding-top:12px">
      <label class="dfield on${st === "wrong" ? " err" : ""}">Invite code<span class="f mono">${st === "wrong" ? "K7QM4-X9TRB" : "K7QM4-X9T"}<span class="caret" style="height:16px"></span></span>
        <span>${st === "wrong" ? "No group has this code. Check it, or ask for a new one." : "Ten letters and digits, like K7QM4-X9TRP. An invite link works too."}</span></label>
      <div style="display:flex;gap:8px;justify-content:flex-end"><button class="dbtn quiet" data-dact="gnew:new">Cancel</button><button class="dbtn pri" data-dact="gnew:join">Find group<kbd>${DI.ret}</kbd></button></div>
    </div>` + dHints([[DI.ret, "find"], ["esc", "back"]])));
    const taken = st === "taken";
    return deskScene(dBar(`<div class="dhead"><span class="davs">${["Sam", "Beep"].map(n => dAv(n)).join("")}</span><span class="tt"><b>Join Tokyo trip?</b><small>Sam and Beep are in it</small></span></div><div class="dform" style="padding-top:12px">
      <div class="dinfo" style="color:var(--d-muted)"><div>Beep can use your memories to help the group. It never shows or quotes them to others.</div><div>When Beep answers you, it runs with your apps and your account, never someone else's.</div><div>You get a notification for each new message.</div></div>
      <label class="dfield on${taken ? " err" : ""}">Your name in the group<span class="f">${taken ? "Sam" : "Dani"}<span class="caret" style="height:16px"></span></span><span>${taken ? "Someone in the group already goes by Sam" : "The others see this name next to your messages"}</span></label>
      <div style="display:flex;gap:8px;justify-content:flex-end"><button class="dbtn" data-dgo="dChats">Not now</button><button class="dbtn pri" data-dgo="dGroup" data-toast="Joined Tokyo trip"${taken ? ' disabled style="opacity:.5"' : ""}>Join group<kbd>${DI.ret}</kbd></button></div>
    </div>` + dHints([[DI.ret, "join"], ["esc", "back"]])));
  },
  // A window like Link to phone, so the code stays up while the bar hides.
  dGroupInvite: () => {
    const reset = S.d.invite === "reset", code = reset ? "HW3NC-8KDQ2" : "K7QM4-X9TRP";
    return deskScene(`<div class="dwin" role="dialog" aria-label="Invite to Tokyo trip"><div class="tb"><span class="tl"><i></i><i></i><i></i></span><span class="name">Invite to Tokyo trip</span></div><div class="in">
      <h4>Invite to Tokyo trip</h4><p>They scan it with Beep on their phone, or enter the code under New group, Join with a code.</p>
      <div class="qr">${qrSvg(reset ? 0x51C3E2 : 0x2F8A69)}</div>
      <div class="code" style="font-size:22px;font-weight:600">${code}</div>
      <div style="display:flex;gap:8px"><button class="dbtn pri" data-toast="Code copied">Copy code</button><button class="dbtn" data-toast="Link copied: https://beep.example.com/g/${code.replace("-", "")}">Copy link</button><button class="dbtn warn" data-dact="inviteReset">Reset code</button></div>
      <p style="font-size:12px">${reset ? "New code made. The old one no longer works. " : ""}Everyone who joins sees the whole chat. Beep never shows one person's memories to the others.</p>
    </div></div>`);
  },
  dGroupInfo: () => {
    const leave = S.d.leave;
    const member = (n, d) => `<div class="row">${dAv(n, "width:28px;height:28px;font-size:12px;margin:0")}<span style="flex:1;min-width:0;display:grid;line-height:18px"><b style="font-weight:500">${n}</b><span style="font-size:12px;color:var(--d-muted)">${d}</span></span></div>`;
    return deskScene(dBar(dGroupHead() + `<div class="dbody" style="padding-left:18px;padding-top:12px;gap:14px">
      <div class="dsec" style="padding:0">Members</div>
      <div class="dinfo">${member("You", "Dani, created the group")}${member("Sam", "Joined 2 Oct")}${member("Beep", "Joins in when it can help, or when someone writes @beep")}</div>
      <div class="dsec" style="padding:0">Reminders</div>
      ${dGroupReminders()}
      <div class="dsec" style="padding:0">Your apps</div>
      <div style="display:flex;gap:12px;align-items:center"><span style="flex:1;display:grid;line-height:19px"><b style="font-weight:500;font-size:13.5px">Share my apps with Beep here</b><span style="font-size:12px;color:var(--d-muted)">${S.d.shareApps ? "Beep reads them to plan for the group and never shows them here. Sending anything asks you first." : "Only when you ask"}</span></span>
        <button class="dswitch" role="switch" aria-checked="${S.d.shareApps}" aria-label="Share my apps with Beep here" data-dact="shareApps"></button></div>
      <div class="dsec" style="padding:0">Privacy</div>
      <div class="dinfo" style="color:var(--d-muted)"><div>Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.</div><div>Beep's thinking and tool calls are hidden in groups. Members only see its replies.</div></div>
      ${leave ? `<div class="dcard"><div style="padding:10px 12px;display:flex;gap:8px;align-items:center;flex-wrap:wrap"><span style="flex:1;font-size:13px">Leave Tokyo trip? You stop getting its messages, and need a new code to come back.</span><button class="dbtn quiet" data-dact="leaveCancel">Cancel</button><button class="dbtn warn" data-dgo="dChats" data-toast="Left Tokyo trip">Leave</button></div></div>`
        : `<div><button class="dbtn warn" data-dact="leaveAsk">Leave group</button></div>`}
    </div>` + dHints([["esc", "back to the chat"], ["⌘K", "chats"]])));
  },
  // Group messages show as banners. A group approval asks the member who wrote, with Allow and Deny on the banner.
  dNotifGroup: () => S.d.gnotif === "approve"
    ? deskScene(`<div class="nbanner" role="alert"><div class="main" data-dgo="dGroup"><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Tokyo trip</b><small>now</small></div><b>Allow Google Calendar: create event?</b><span>You asked Beep in Tokyo trip. Dinner in Asakusa, Thu 14 Nov 19:30</span></div></div>
      <div class="side"><button data-dact="gAllow">Allow</button><button data-dact="gDeny">Deny</button></div></div>`, { badge: true })
    : deskScene(`<div class="nbanner" role="alert"><div class="main" data-dgo="dGroup"><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Tokyo trip</b><small>now</small></div><span><b>Beep:</b> Two quiet ones near Asakusa, both with private baths and under ¥40,000 a night…</span></div></div></div>`),
});

// The group's reminders on the desktop group info: Beep posts each into the chat when it is due.
function dGroupReminders() {
  const ask = 'Ask Beep in the chat to add one, like "remind us at 10 to check out".';
  if (S.d.gRem === "empty") return `<div class="dinfo" style="color:var(--d-muted)">No reminders yet. ${ask}</div>`;
  const rows = [["Hotel check-out", "Check-out at 11. Leave the keys at the front desk.", "Tomorrow 10:00", "Sam"],
    ["Dinner plans", "Who's in for dinner tonight? Reply here by 6.", "Every day at 17:00", "you"]];
  return `<div class="dinfo">${rows.map(([label, msg, when, by]) => `<div class="row" style="align-items:flex-start">
    <span style="flex:1;min-width:0;display:grid;line-height:18px"><b style="font-weight:500">${label}</b><span style="font-size:12px;color:var(--d-muted)">${msg}</span><span style="font-size:12px;color:var(--d-muted)">${when} · set by ${by}</span></span>
    <button class="dbtn warn" data-toast="Reminder deleted" aria-label="Delete reminder">Delete</button></div>`).join("")}
    <div style="font-size:12px;color:var(--d-muted)">${ask}</div></div>`;
}
const IC = {
  up: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>',
  stop: '<span style="width:12px;height:12px;border-radius:3px;background:currentColor;display:block"></span>',
  image: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9" stroke-linecap="round"/></svg>',
  ne: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 17L17 7M8 7h9v9"/></svg>',
  gt: '<svg width="8" height="14" viewBox="0 0 8 14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 1l6 6-6 6"/></svg>',
  pinf: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  send: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 3L10 14M21 3l-7 18-4-7-7-4 18-7z"/></svg>',
};
Object.assign(S, { home: "list", save: "saved", radius: "100", places: "list", trig: "arrive", trigPlace: "Gym", trigRepeat: "daily", arrive: "arrived", bgLoc: "ask", thread: "runs", ginfo: "info", creator: true });
const vs = (key, list) => list.map(([label, val]) => [label, () => { S[key] = val; render(); }, () => S[key] === val]);
