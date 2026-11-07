// ---- Group chats ----
const MEMBERS = [["You", "D", "var(--m-t2)"], ["Sam", "S", "var(--m-t0)"], ["Beep", "b", "var(--m-brand)"]];
const av = (ini, bg, st = "") => `<span class="av" style="background:${bg};${ini === "b" ? "color:#fff;" : ""}${st}">${ini === "b" ? BI(18) : ini}</span>`;
// Same as a normal chat: your messages on the right, Beep's replies full width, members in a bubble on the left with their name.
const gMsg = (who, ini, bg, color, html) => who === "Beep"
  ? `<div class="gm beep"><span class="who" style="color:${color}">Beep</span><div class="b">${html}</div></div>`
  : `<div class="gm"><div class="b"><span class="who" style="color:${color}">${who}</span>${html}</div></div>`;
function groupBar() {
  return `<div class="bar" style="padding-left:6px;gap:8px">
    <button class="ib" data-go="chat" aria-label="Chats">${I.back}</button>
    <button style="flex:1;display:grid;gap:3px;border:0;background:transparent;text-align:left;padding:0" data-go="groupInfo">
      <div class="title" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">Tokyo trip</div>
      <div class="sub"><span class="avs">${MEMBERS.map(([, i, bg]) => av(i, bg)).join("")}</span>You, Sam and Beep</div>
    </button>
    <button class="ib muted" data-go="groupInvite" aria-label="Invite">${I.link}</button>
  </div>`;
}
function groupBody(extra = "") {
  const typing = S.group === "typing", casual = S.group === "casual", swiping = S.groupSwipe;
  const hint = swiping ? `<div style="position:absolute;left:16px;top:45%;display:grid;justify-items:center;gap:6px;z-index:0"><span style="width:44px;height:44px;border-radius:50%;background:var(--m-accent);border:2px solid var(--m-ledge);display:grid;place-items:center">${I.back}</span><span style="font-size:11px;font-weight:600;color:var(--m-muted)">Previous</span></div>` : "";
  return hint + `<div class="body" style="gap:18px;padding:8px 18px 16px;${swiping ? "transform:translateX(96px);opacity:.75;position:relative;z-index:1;background:var(--m-page)" : ""}">
    ${iDayA("Yesterday")}
    ${gMsg("Sam", "S", "var(--m-t0)", "#B8574B", "Flights are booked! Landing at Haneda on the 14th, 16:20")}
    <div class="user">Nice. Three nights in Tokyo then Kyoto?</div>
    ${iDayA("Today")}
    ${gMsg("Sam", "S", "var(--m-t0)", "#B8574B", `<span class="mention">@beep</span> can you find us a ryokan for the 14th to the 17th? Something quiet`)}
    ${casual ? gMsg("Sam", "S", "var(--m-t0)", "#B8574B", "Where should we have dinner tonight? Somewhere near the hotel") +
      `<div class="user">Not sushi again pls</div>` +
      gMsg("Beep", "b", "var(--m-brand)", "var(--m-brand)", `Three near Asakusa, none of them sushi:<br><br><b>Asakusa Imahan</b>: sukiyaki, 4 minutes away, book ahead.<br><b>Daikokuya</b>: tempura, no bookings, usually a short queue after 20:00.<br><b>Kappo Ohashi</b>: small, seasonal, a vegetarian set if you ask.<br><br>Want me to check a table for three at 20:00?`) :
    typing ? `<div class="sub"><span class="typing"><i></i><i></i><i></i></span>Beep is typing</div>` :
    gMsg("Beep", "b", "var(--m-brand)", "var(--m-brand)", `Two quiet ones near Asakusa, both with private baths and under ¥40,000 a night:<br><br><b>Sadachiyo</b>: old wooden inn, kaiseki dinner with a vegetarian set.<br><b>Kamogawa Asakusa</b>: a five minute walk from Senso-ji, early breakfast from 6:30.<br><br>Both have rooms free on those nights. Want me to hold one?`) +
    `<div style="display:flex;gap:6px 8px;flex-wrap:wrap">
      <button class="chunky rchip" data-toast="Sent: @beep hold Sadachiyo">Hold Sadachiyo</button>
      <button class="chunky rchip" data-toast="Sent: @beep show more">Show more</button></div>`}
    ${extra}
  </div>`;
}
const iDayA = d => `<div style="align-self:stretch;text-align:center;font-size:11px;color:var(--m-dim)">${d}</div>`;
// The normal chat's composer, with a hint about @beep.
function groupComposer(text) {
  return `<div class="composer"><div class="input" style="${text ? "color:var(--m-ink)" : ""}">${text || "Message"}${text ? '<span class="caret" style="height:18px;margin-left:1px"></span>' : ""}</div><button class="round coral" aria-label="${text ? "Send" : "Talk"}">${text ? I.send : I.mic}</button></div>`;
}
Object.assign(R, {
  groupChat: () => groupBar() + groupBody() + groupComposer() + (S.groupNotif ? `<div class="notif" data-act="notifOpen" style="cursor:pointer">
      <span class="logo" style="width:36px;height:36px;border-radius:var(--radius);background:#0B0E1D;color:#fff">${BI(20)}</span>
      <div class="grow"><b style="font-size:14px">Tokyo trip</b><span style="font-size:13px;color:var(--m-muted);line-height:18px">Beep: Two quiet ones near Asakusa, both with private baths and under ¥40,000 a night…</span></div>
      <span style="font-size:11px;color:var(--m-dim)">now</span></div>` : "") + (S.groupAsk ? `<div class="notif" style="flex-direction:column;gap:6px">
      <div class="row" style="align-items:center;gap:10px"><span class="logo" style="width:36px;height:36px;border-radius:var(--radius);background:#0B0E1D;color:#fff">${BI(20)}</span>
      <div class="grow"><b style="font-size:14px">Allow Google Calendar: create event?</b><span style="font-size:13px;color:var(--m-muted);line-height:18px">Sam asked Beep in Tokyo trip. Dinner in Asakusa, Thu 14 Nov 19:30</span></div></div>
      <div class="end" style="gap:4px"><button class="text-btn" data-act="groupDeny">Deny</button><button class="text-btn" style="color:var(--m-ok)" data-act="groupAllow">Allow</button></div></div>` : ""),
  mention: () => groupBar() + groupBody() + `<div class="mlist" role="listbox" aria-label="Mention">
      <button data-toast="@beep added">${av("b", "var(--m-brand)")}<span><b style="font-weight:600">Beep</b><small>Make sure Beep replies</small></span></button>
      <button data-toast="@Sam added">${av("S", "var(--m-t0)")}<span><b style="font-weight:500">Sam</b><small>Member</small></span></button>
    </div>` + groupComposer("@"),
  groupNew: () => R.drawer() + `<div class="scrim" data-go="drawer" style="z-index:8"></div>
    <div class="sheet" role="dialog" aria-label="New group">
      <div class="grab"></div>
      <div class="row" style="align-items:center"><span class="logo" style="width:52px;height:52px;border-radius:var(--radius);background:var(--m-t2)">${I.group}</span>
        <div class="grow"><h3>New group</h3><span class="muted" style="font-size:13px">Chat with people, and Beep helps when it can</span></div></div>
      <label class="fl">Group name<div class="field" style="display:flex;align-items:center;color:var(--m-ink)">Tokyo trip<span class="caret" style="height:18px;margin-left:1px"></span></div></label>
      <label class="fl">Your name in the group<div class="field" style="display:flex;align-items:center;color:var(--m-ink)">Dani</div></label>
      <div class="muted" style="font-size:12px;line-height:18px">You get a code to send to people. Anyone with Beep who enters it can join.</div>
      <div class="end" style="justify-content:space-between"><button class="text-btn" style="padding:0" data-go="groupCode">Join with a code</button><span style="display:flex;gap:8px"><button class="text-btn" data-go="drawer">Cancel</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-go="groupInvite">Create</button></span></div>
    </div>`,
  // Entering a code someone sent; dashes and case do not matter.
  groupCode: () => R.drawer() + `<div class="scrim" data-go="drawer" style="z-index:8"></div>
    <div class="sheet" role="dialog" aria-label="Join with a code">
      <div class="grab"></div>
      <div class="row" style="align-items:center"><span class="logo" style="width:52px;height:52px;border-radius:var(--radius);background:var(--m-t1)">${I.group}</span>
        <div class="grow"><h3>Join a group</h3><span class="muted" style="font-size:13px">Enter or scan the code someone sent you</span></div></div>
      <label class="fl">Invite code<div class="field" style="display:flex;align-items:center;font-family:var(--mono);font-size:18px;letter-spacing:2px;color:var(--m-ink);${S.code === "wrong" ? "border-color:var(--m-error)" : ""}">${S.code === "wrong" ? "K7QM4-X9TRB" : "K7QM4-X9T"}<span class="caret" style="height:20px;margin-left:1px"></span></div>
        ${S.code === "wrong" ? `<span style="color:var(--m-error)">No group has this code. Check it, or ask for a new one.</span>` : `<span>Ten letters and digits, like K7QM4-X9TRP</span>`}</label>
      <button class="pill chunky" style="display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-act="scanGroup">${I.qrcode}Scan a QR code</button>
      <div class="end"><button class="text-btn" data-go="drawer">Cancel</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-go="groupJoin">Find group</button></div>
    </div>`,
  groupInvite: () => {
    const key = S.invite === "reset" ? "HW3NC-8KDQ2" : "K7QM4-X9TRP";
    return groupBar() + groupBody() + groupComposer() + `<div class="scrim" data-go="groupChat" style="z-index:8"></div>
    <div class="sheet" role="dialog" aria-label="Invite to group">
      <div class="grab"></div>
      <div class="row" style="align-items:center"><span class="logo" style="width:52px;height:52px;border-radius:var(--radius);background:var(--m-t1)">${I.link}</span>
        <div class="grow"><h3>Invite people</h3><span class="muted" style="font-size:13px">Tokyo trip</span></div></div>
      <div class="qr" style="justify-self:center;align-self:center;margin:0 auto;width:180px;height:180px">${qrSvg(S.invite === "reset" ? 0x51C3E2 : 0x2F8A69)}</div>
      <div class="field" style="display:flex;align-items:center;justify-content:center;height:56px;font-family:var(--mono);font-size:26px;font-weight:600;letter-spacing:3px;color:var(--m-ink)">${key}</div>
      <div style="display:flex;gap:10px">
        <button class="pill chunky" style="flex:1;display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-toast="Link copied">${I.copy}Copy link</button>
        <button class="pill chunky coral" style="flex:1;display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-toast="Share menu: Join Tokyo trip on Beep: https://app.example.com/g/${key.replace('-', '')}">${I.share}Send link</button>
      </div>
      <div class="muted" style="font-size:12px;line-height:18px">${S.invite === "reset" ? "New code made. The old one no longer works. " : ""}People open the link or scan the code, or enter the code under New group, Join with a code, and see the whole chat. Beep uses what it knows about each of you to help, but never shows one person's memories to the others.</div>
      <div class="end" style="justify-content:space-between"><button class="text-btn" style="color:var(--m-error);padding:0" data-act="inviteReset">Reset code</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-go="groupChat">Done</button></div>
    </div>`;
  },
  groupJoin: () => {
    if (S.join === "expired") return `<div class="body" style="justify-content:center;align-items:center;text-align:center;gap:14px;padding:24px">
      <span class="logo" style="width:64px;height:64px;border-radius:var(--radius);background:var(--m-t0)">${I.link}</span>
      <div class="big" style="font-size:30px">This code no longer works</div>
      <div class="muted" style="max-width:260px">Whoever sent it reset the group's code. Ask them for a new one.</div>
      <button class="primary" style="margin-top:12px" data-go="empty">Back to Beep</button></div>`;
    return `<div class="body" style="justify-content:center;gap:16px;padding:24px">
      <span class="avs" style="align-self:center">${[["D", "var(--m-t2)"], ["S", "var(--m-t0)"], ["b", "var(--m-brand)"]].map(([i, bg]) => av(i, bg, "width:56px;height:56px;font-size:20px;margin-left:-14px")).join("")}</span>
      <div style="text-align:center;display:grid;gap:6px"><div class="big" style="font-size:32px">Join Tokyo trip?</div>
        <div class="muted">Sam invited you. Sam and Beep are in it.</div></div>
      <div class="card" style="display:grid;gap:10px;font-size:13px;line-height:19px">
        <div class="row">${I.brain}<span>Beep can use your memories to help the group. It never shows or quotes them to others.</span></div>
        <div class="row">${I.shield}<span>Beep reads the messages to see when it can help. When it answers you, it runs with your apps and your account, never someone else's.</span></div>
        <div class="row">${I.bell}<span>You get a notification for each new message.</span></div>
      </div>
      <label class="fl">Your name in the group<div class="field" style="display:flex;align-items:center;color:var(--m-ink);${S.join === "taken" ? "border-color:var(--m-error)" : ""}">${S.join === "taken" ? "Sam" : "Dani"}<span class="caret" style="height:18px;margin-left:1px"></span></div>
        ${S.join === "taken" ? `<span style="color:var(--m-error)">Someone in the group already goes by Sam</span>` : `<span>The others see this name next to your messages</span>`}</label>
      <button class="primary" data-act="joinDo"${S.join === "taken" ? ' disabled style="opacity:.5"' : ""}>Join group</button>
      <button class="text-btn" style="align-self:center" data-go="empty">Not now</button></div>`;
  },
  groupInfo: () => back("Group info", "").replace('data-go="chat"', 'data-go="groupChat"') + `<div class="body">
      <div class="label">Members</div>
      <div class="card" style="display:grid;gap:12px;padding:12px 14px">
        ${[["You", "D", "var(--m-t2)", "Created the group"], ["Sam", "S", "var(--m-t0)", "Joined 2 Oct"], ["Beep", "b", "var(--m-brand)", "Joins in when it can help, or when someone writes @beep"]].map(([n, i, bg, d]) => `<div class="row" style="align-items:center">${av(i, bg, "width:36px;height:36px;font-size:13px")}<div class="grow"><b style="font-weight:500">${n}</b><span class="muted" style="font-size:12px">${d}</span></div></div>`).join("")}
        <button class="pill chunky" style="display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-go="groupInvite">${I.link}Invite with a code</button>
      </div>
      <div class="label">Reminders</div>
      ${groupReminders()}
      <div class="label">Your apps</div>
      <div class="card" style="display:grid;gap:8px;padding:12px 14px">
        <div class="row" style="align-items:center"><div class="grow"><b style="font-weight:500">Share my apps with Beep here</b><span class="muted" style="font-size:12px;line-height:17px">${S.shareApps ? "Beep can read your calendar, mail, files and tasks to plan for the group, and never shows them here. Creating or sending anything asks you first." : "Beep uses your apps here only when you ask it yourself."}</span></div>
        <button class="switch" role="switch" aria-checked="${S.shareApps}" aria-label="Share my apps with Beep here" data-act="shareApps"></button></div>
      </div>
      <div class="label">Privacy</div>
      <div class="card" style="display:grid;gap:10px;font-size:13px;line-height:19px">
        <div class="row">${I.brain}<span>Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.</span></div>
        <div class="row">${I.shield}<span>Beep's thinking and tool calls are hidden in groups. Members only see its replies.</span></div>
      </div>
      <button class="text-btn" style="color:var(--m-error);align-self:flex-start" data-toast="Leave Tokyo trip?">Leave group</button>
    </div>${S.gRem === "deleted" ? `<div class="snack">Reminder deleted</div>` : ""}`,
});

// The group's reminders in Group info. Beep posts each into the chat when it is due.
function groupReminders() {
  const ask = 'Ask Beep in the chat to add one, like "remind us at 10 to check out".';
  if (S.gRem === "empty") return `<div class="card" class="muted" style="font-size:13px;line-height:19px">No reminders yet. ${ask}</div>`;
  const rows = [
    ["Hotel check-out", "Check-out at 11. Leave the keys at the front desk.", "Tomorrow 10:00", "Sam", "var(--m-t1)"],
    ["Shinkansen", "Bring passports and JR passes.", "Fri 7:30", "you", "var(--m-t2)"],
    ["Dinner plans", "Who's in for dinner tonight? Reply here by 6.", "Every day at 17:00", "Sam", "var(--m-t0)"],
  ].filter((_, i) => !(S.gRem === "deleted" && i === 1));
  return `<div class="card" style="display:grid;gap:12px;padding:12px 14px">
    ${rows.map(([label, msg, when, by, bg]) => `<div class="row" style="align-items:center">
      <span style="width:36px;height:36px;border-radius:var(--radius);border:1.5px solid var(--m-ledge);background:${bg};display:grid;place-items:center;flex:none">${I.clock}</span>
      <div class="grow"><b style="font-weight:500">${label}</b><span class="muted" style="font-size:12px;line-height:17px">${msg}</span><span class="muted" style="font-size:12px">${when} · set by ${by}</span></div>
      <button class="ib" style="width:36px;height:36px;color:var(--m-muted)" data-act="gRemDel" aria-label="Delete reminder">${I.x}</button></div>`).join("")}
    <span class="muted" style="font-size:12px;line-height:17px">${ask}</span>
  </div>`;
}
