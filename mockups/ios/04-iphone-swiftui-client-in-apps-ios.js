// ---- iPhone (SwiftUI client in apps/ios) ----
const IOS_SCREENS = [
  { group: "Onboarding", items: [["iOnboard", "Welcome", "OnboardingView"]] },
  { group: "Chat", items: [
    ["iEmpty", "New chat", "ChatView, empty"],
    ["iChat", "Reply with tools", "ChatView"],
    ["iApprove", "Approve a tool", "ApprovalCard"],
    ["iChats", "Chats", "ChatDrawer"],
  ]},
  { group: "Group chats", items: [
    ["iGroup", "Group chat", "GroupChatView"],
    ["iGroupNew", "New group", "NewGroupSheet"],
    ["iGroupInvite", "Invite link", "GroupInviteSheet"],
    ["iGroupCode", "Join with a link", "JoinGroupSheet"],
    ["iGroupJoin", "Join a group", "GroupJoinView"],
    ["iGroupInfo", "Group info", "GroupInfoView"],
  ]},
  { group: "Settings", items: [
    ["iSettings", "Settings", "SettingsView"],
    ["iShowCode", "Show a code", "ShowCodeSheet"],
    ["iMemories", "Memories", "MemoriesView"],
    ["iRoutines", "Routines", "RoutinesView"],
    ["iNewRoutine", "New routine", "NewRoutineView"],
  ]},
  { group: "Widgets", items: [["iWidget", "Ask Beep widget", "BeepWidget"]] },
];
const IC = {
  up: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>',
  stop: '<span style="width:12px;height:12px;border-radius:3px;background:currentColor;display:block"></span>',
  image: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9" stroke-linecap="round"/></svg>',
  ne: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 17L17 7M8 7h9v9"/></svg>',
  gt: '<svg width="8" height="14" viewBox="0 0 8 14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 1l6 6-6 6"/></svg>',
  pinf: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  send: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 3L10 14M21 3l-7 18-4-7-7-4 18-7z"/></svg>',
};
// The same top bar as Android: chunky menu button, Beep with the connection dot under it.
const iChatBar = () => `<div class="bar">
  <button class="ib chunky" style="width:42px;height:42px" data-igo="iChats" aria-label="Conversations">${I.menu}</button>
  <div style="flex:1;min-width:0;display:grid;gap:3px"><div style="font-weight:600;font-size:16px;line-height:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${{ iChat: "Saturday plan", iApprove: "Lunch with Sam" }[S.screen] || "New conversation"}</div>
    <div class="sub"><span class="dot ${S.i.online ? "" : "off"}"></span>${S.i.online ? "Connected" : '<span data-iact="reconnect" style="cursor:pointer">Tap to reconnect</span>'}</div></div>
  <button class="ib muted" data-igo="iLiveLaunch" aria-label="Talk live">${IC.wave}</button>
  <button class="ib muted" data-igo="iEmpty" aria-label="New chat">${I.plus}</button>
</div>`;
const iBack = (title, to, extra = "") => `<div class="bar" style="padding-left:6px"><button class="ib chunky" data-igo="${to}" aria-label="Back">${I.back}</button><div class="title" style="flex:1">${title}</div>${extra}</div>`;
// Field and button are both 50 tall with radius 18. The photo button sits in the field until something is typed.
// The button is a faded paper plane when empty, a coral one with text, stop while running.
const iComposer = (o = {}) => {
  const btn = o.running ? `<button class="ib chunky icbtn" style="background:var(--m-ink);color:var(--m-page)" data-iact="stop" aria-label="Stop">${IC.stop}</button>`
    : o.text ? `<button class="ib chunky icbtn coral" aria-label="Send">${IC.send}</button>`
    : `<button class="ib chunky icbtn" style="opacity:.45" disabled aria-label="Send">${IC.send}</button>`;
  const photo = o.text || o.running ? "" : `<button class="ib" style="width:36px;height:36px;color:var(--m-muted)" data-igo="iCamera" aria-label="Add a photo">${IC.image}</button>`;
  return `<div class="icomp"><div class="ifield">${photo}<span style="flex:1;color:var(${o.text ? "--m-ink" : "--m-dim"})">${o.text || "Ask anything..."}</span></div>${btn}</div>`;
};
const iChip = (label, ini, st, i) => `<div><span class="chip" style="background:${TINTS[i % 3]}"><span class="ini${st === "run" ? " wiggle" : ""}">${ini}</span>${label}<span class="st">${st === "run" ? '<span class="spin"></span>' : st === "bad" ? `<span style="color:var(--m-error);display:grid">${I.x}</span>` : `<span style="color:var(--m-ok);display:grid">${I.check}</span>`}</span></span></div>`;
const iFold = (n, apps, open) => `<button class="fold on" data-iact="fold" aria-expanded="${open}"><span class="stack">${apps.map((a, k) => `<i style="background:${TINTS[k % 3]};z-index:${k}">${a}</i>`).join("")}</span>+${n} tools${I.chev.replace('class="chev"', `class="chev" style="transform:rotate(${open ? 180 : 0}deg)"`)}</button>`;
const iUnder = `<div style="display:flex;gap:4px;margin-top:-6px"><button class="text-btn" style="color:var(--m-muted);font-size:11px;min-height:28px;padding:0 6px" data-itoast="Copied">Copy reply</button><button class="text-btn" style="color:var(--m-muted);font-size:11px;min-height:28px;padding:0 6px" data-itoast="Quoted in the composer">Quote</button></div>`;

// Scan or type the code a computer shows, then link it or join its account (LinkCodes.swift).
const iComputer = () => `<div class="card" style="display:grid;gap:12px">
  <div class="row" style="align-items:center"><div class="logo" style="background:${TINTS[1]}">${S.i.computer ? I.check : '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="5" width="16" height="11" rx="2"/><path d="M2 19h20"/></svg>'}</div>
    <div class="grow"><b style="font-weight:600">${S.i.computer ? "On sample-account's account" : "Using Beep on a computer?"}</b><span class="muted" style="font-size:13px;line-height:19px">${S.i.computer ? "This phone now shows sample-account's chats and memories." : "Scan the code Beep shows on your computer to use the same chats and memories here."}</span></div></div>
  ${S.i.computer ? "" : `<div style="display:flex;gap:8px;align-items:center"><button class="pill" style="min-height:36px;padding:0 14px;background:var(--m-user)" data-iact="computer">Scan code</button><button class="text-btn" style="color:var(--m-muted)" data-iact="computer">Type code</button><button class="text-btn" style="color:var(--m-muted)" data-iact="showcode">Show code</button></div>`}
</div>`;

const P = {
  iOnboard: () => {
    const card = (k, icon, title, desc, tint) => `<div class="card row" style="align-items:center">
      <div class="logo" style="background:${tint}">${icon}</div>
      <div class="grow"><b style="font-weight:600">${title}</b><span class="muted" style="font-size:13px;line-height:19px">${desc}</span></div>
      ${S.i.perms[k] ? `<span style="color:var(--m-ok)">${I.check}</span>` : `<button class="pill" style="min-height:36px;padding:0 12px" data-iperm="${k}">Allow</button>`}</div>`;
    return `<div class="body" style="padding:24px 20px 12px;gap:16px">
      <div class="chunky" style="width:56px;height:56px;border-radius:var(--radius);background:var(--m-user);display:grid;place-items:center;font-family:var(--display);font-weight:800;font-size:24px">${BI(28)}</div>
      <div class="big">Almost ready.</div>
      <div class="muted" style="line-height:22px">You are <b style="color:var(--m-ink);font-weight:600">${S.i.imported ? "sample-account" : "sample-account"}</b>. Beep made a key for this iPhone and keeps it in the Keychain. Both permissions are optional.</div>
      ${card("loc", I.pin, "Location", "Nearby places, weather and directions.", TINTS[0])}
      ${card("notif", I.bell, "Notifications", "When a routine finishes or a tool needs your OK.", TINTS[1])}
      <div class="card row" style="align-items:center">
        <div class="logo" style="background:${TINTS[2]}">${I.key}</div>
        <div class="grow"><b style="font-weight:600">${S.i.imported ? "Signed in as sample-account" : "Already use Beep?"}</b><span class="muted" style="font-size:13px;line-height:19px">${S.i.imported ? "From the key file. It replaces the new account." : "Import the key file saved from Settings on your other phone."}</span></div>
        ${S.i.imported ? `<span style="color:var(--m-ok)">${I.check}</span>` : `<button class="pill" style="min-height:36px;padding:0 12px" data-iact="import">Import</button>`}</div>
      ${iComputer()}
      <div style="flex:1"></div>
      <button class="primary" data-igo="iEmpty">Continue</button>
    </div>`;
  },
  iEmpty: () => {
    const items = [["Remember something", I.brain], ["Plan my Saturday", I.clock], ["Inbox check", I.search]];
    return iChatBar() + `<div class="body" style="padding:28px 18px 16px">
      <div class="big" style="font-size:40px;line-height:42px">A little help.<br><span style="color:var(--m-ok)">Right here.</span></div>
      <div class="muted" style="line-height:23px;margin-bottom:16px">Find an answer. Untangle a thought.<br>Keep something worth remembering.</div>
      ${items.map(([t, ic], i) => `<button class="chunky sugg" style="border-radius:var(--radius);animation-delay:${120 + i * 90}ms" data-igo="iChat"><span class="ico" style="width:34px;height:34px;border-radius:var(--radius);background:${TINTS[i % 3]}">${ic}</span><span style="min-width:0;flex:1"><b style="font-weight:500;font-size:15px">${t}</b></span><span class="muted" style="display:grid">${IC.ne}</span></button>`).join("")}
      <button class="text-btn" style="align-self:center;color:var(--m-muted);font-size:13px" data-itoast="New ideas">New ideas</button>
    </div>` + iComposer({ text: S.i.typing ? "What's on tomorrow?" : "" });
  },
  iChat: () => {
    const run = S.i.run === "running", stopped = S.i.run === "stopped", open = S.i.fold;
    const tools = [["Checked the time", "T", "ok"], ["Found your location", "L", "ok"], ["Checked the weather", "W", "ok"], ["Read your calendar", "C", "ok"], ["Found free time", "C", run ? "run" : stopped ? "bad" : "ok"]];
    const shown = open ? tools : tools.slice(-3);
    return iChatBar() + `<div class="body" style="padding:8px 18px 16px;gap:18px">
      <div class="user" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius) 18px 6px 18px;margin-left:48px">Plan my Saturday around the rain</div>
      <div style="display:flex;flex-direction:column">
        ${S.i.thoughts !== "none" ? `<button class="text-btn" style="align-self:flex-start;color:var(--m-muted);font-size:12px;padding:0 4px;min-height:28px;display:flex;align-items:center;gap:2px" data-iact="thoughts">Thoughts${I.chev.replace('class="chev"', `class="chev" style="transform:rotate(${S.i.thoughts === "open" ? 180 : 0}deg)"`)}</button>` : ""}
        ${S.i.thoughts === "open" ? `<div class="thoughts">Rain first, then the calendar. <b>Lunch with Sam</b> is fixed at 13:00, so the walk fits between.</div>` : ""}
        ${iFold(2, ["T", "L"], open)}
        ${shown.map(([l, n, st], i) => `<div style="margin-top:6px">${iChip(l, n, st, i + (open ? 0 : 2))}</div>`).join("")}
      </div>
      ${run ? `<div style="display:flex;align-items:center;gap:8px;font-size:13px;color:var(--m-muted)"><span class="spin" style="border-top-color:var(--m-accent)"></span>Finding free time</div>`
        : S.i.run === "table" ? `<div class="reply"><b>Saturday is wet until about 11:00</b>, then dry.<div class="itable"><table><thead><tr><th>Time</th><th>Plan</th><th>Weather</th><th>Where</th></tr></thead><tbody><tr><td><b>09:30</b></td><td>Coffee and pharmacy</td><td>Rain, 14°C</td><td>High Street</td></tr><tr><td><b>11:30</b></td><td>River walk</td><td>Dry, 17°C</td><td>Riverside path</td></tr><tr><td><b>13:00</b></td><td>Lunch with Sam</td><td>Dry, 19°C</td><td>Café Lume</td></tr></tbody></table></div>Want me to block the walk in your calendar?</div>${iUnder}`
        : stopped ? `<div style="padding:4px 6px 4px 14px;border-radius:var(--radius);background:color-mix(in srgb, var(--m-error) 10%, transparent);display:flex;align-items:center;gap:8px"><div style="flex:1;font-size:13px;color:var(--m-error);padding:8px 0">Stopped</div><button class="text-btn" style="color:var(--m-error);font-weight:600" data-iact="retry">Retry</button></div>`
        : `<div class="reply"><b>Saturday is wet until about 11:00</b>, then dry and around 19°C.<ul style="margin:6px 0;padding-left:20px"><li><b>09:30</b> coffee and the pharmacy run</li><li><b>11:30</b> walk along the river</li><li><b>13:00</b> lunch with Sam</li></ul>Want me to block the walk in your calendar?</div>${iUnder}`}
    </div>` + iComposer({ running: run });
  },
  iApprove: () => {
    const st = S.i.approve, ask = st === "ask";
    const card = ask ? `<div class="ask" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius)">
        <div class="head"><span class="logo" style="width:32px;height:32px;border-radius:9px;background:var(--m-surface)">F</span><div class="grow"><b style="font-weight:600">Send email with Fastmail?</b><span class="muted" style="font-size:12px">Fastmail asks before sending</span></div></div>
        <div class="kv"><span>To</span><span>Sam Rivera</span><span>Subject</span><span>Lunch on Saturday</span></div>
        <div class="draft">Hi Sam, can we move lunch on Saturday to 13:30? Same place. Thanks!</div>
        <div class="acts"><button class="go" style="box-shadow:0 3px 0 var(--m-ledge);background:var(--m-ok);color:#fff" data-iapprove="allowed">Allow</button><button class="pill" data-iapprove="always">Always</button><span style="flex:1"></span><button class="text-btn" style="color:var(--m-error)" data-iapprove="denied">Deny</button></div>
      </div>` : `<div class="sub">${I.shield}${{ allowed: "Allowed once", always: "Always allowed for Fastmail", denied: "Denied" }[st]}</div>`;
    const after = { ask: "", allowed: "Sent. Sam has it.", always: "Sent. Sam has it.", denied: "Okay, I didn't send it. Want me to change the wording?" }[st];
    return iChatBar() + `<div class="body" style="padding:8px 18px 16px;gap:18px">
      <div class="user" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius) 18px 6px 18px;margin-left:48px">Tell Sam lunch on Saturday moves to 13:30</div>
      <div style="display:flex;flex-direction:column;gap:6px">${iChip("Searched Fastmail", "F", "ok", 0)}${iChip(ask ? "Sending email" : st === "denied" ? "Send email" : "Sent email", "F", ask ? "run" : st === "denied" ? "bad" : "ok", 1)}</div>
      <div class="reply">Here is the email. It goes out once you allow it.</div>
      ${card}${after ? `<div class="reply">${after}</div>` : ""}
    </div>` + iComposer({ running: ask });
  },
  // WidgetKit: the Android "Ask Beep..." bar on the Home Screen, small and medium, and on the Lock Screen.
  // Any tap opens a new chat with the keyboard up (beep://new).
  iWidget: () => {
    const tile = (size, r) => `<div style="width:${size}px;height:${size}px;border-radius:${r}px;background:#0B0E1D;border:1.5px solid var(--m-ledge);box-shadow:2px 2px 0 var(--m-ledge);display:grid;place-items:center;font-family:var(--display);font-weight:800;font-size:${size * 0.5}px;color:#fff;flex:none">${BI(Math.round(size * 0.5))}</div>`;
    const field = `<div style="flex:1;min-width:0;display:flex;align-items:center;gap:8px;min-height:44px;padding:0 14px;border-radius:999px;background:var(--m-surface);border:1.5px solid var(--m-ledge);box-shadow:2px 2px 0 var(--m-ledge);color:var(--m-dim);font-size:15px">Ask Beep...</div>`;
    const app = (c) => `<div style="display:grid;justify-items:center;gap:4px"><i style="width:56px;height:56px;border-radius:var(--radius);background:${c};display:block"></i><span style="width:40px;height:6px;border-radius:3px;background:rgba(255,255,255,.55)"></span></div>`;
    if (S.i.widget === "lock") return `<div style="position:absolute;inset:0;background:linear-gradient(170deg,#3C6B5A,#14231D);color:#fff;display:flex;flex-direction:column;align-items:center;padding:48px 20px 0">
      <div style="font-size:15px;font-weight:500;opacity:.85">Saturday 17 October</div>
      <div style="font-family:var(--body);font-weight:600;font-size:86px;line-height:1;letter-spacing:-0.02em;margin-top:2px">9:41</div>
      <div style="display:flex;gap:12px;margin-top:14px;align-items:center">
        <button style="width:64px;height:64px;border-radius:50%;border:0;background:rgba(255,255,255,.18);backdrop-filter:blur(12px);display:grid;place-items:center;color:#fff;font-family:var(--display);font-weight:800;font-size:28px" data-iact="widget" aria-label="Ask Beep">${BI(30)}</button>
        <button style="width:160px;height:64px;border-radius:var(--radius);border:0;background:rgba(255,255,255,.18);backdrop-filter:blur(12px);display:grid;align-content:center;justify-items:start;padding:0 14px;color:#fff;text-align:left" data-iact="widget">
          <b style="font-weight:600;font-size:14px">Beep</b><span style="font-size:13px;opacity:.8">Ask anything</span></button>
      </div>
    </div>`;
    return `<div style="position:absolute;inset:0;background:linear-gradient(165deg,#9CC8B5,#E9C6B8);padding:46px 22px 0;display:flex;flex-direction:column;gap:22px">
      <button style="border:0;padding:14px;border-radius:var(--radius);background:var(--m-page);display:flex;align-items:center;gap:12px;text-align:left;box-shadow:0 2px 10px rgba(16,32,26,.15)" data-iact="widget" aria-label="Ask Beep">${tile(40, 12)}${field}</button>
      <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:20px 0;justify-items:center">${["#F4B860", "#6FA8DC", "#E06666", "#93C47D", "#8E7CC3", "#FFD966", "#76A5AF", "#C27BA0"].map(app).join("")}</div>
      <div style="display:flex;gap:22px;align-items:flex-start">
        <button style="width:150px;height:150px;flex:none;border:0;padding:14px;border-radius:var(--radius);background:var(--m-page);display:flex;flex-direction:column;justify-content:space-between;text-align:left;box-shadow:0 2px 10px rgba(16,32,26,.15)" data-iact="widget" aria-label="Ask Beep">
          ${tile(44, 13)}<div style="display:grid;gap:2px"><div class="title" style="font-size:20px">Beep</div><span style="font-size:13px;color:var(--m-muted)">Ask anything</span></div></button>
        <div style="display:grid;grid-template-columns:repeat(2,1fr);gap:20px 22px">${["#6D9EEB", "#F6B26B", "#A4C2F4", "#B6D7A8"].map(app).join("")}</div>
      </div>
    </div>`;
  },
  // A code for a new phone or computer to join this account with (LinkCodes.swift).
  iShowCode: () => P.iSettings() + `<div class="scrim" data-go="iSettings" style="z-index:8"></div>
    <div class="sheet" role="dialog" aria-label="Show a code">
      <div class="grab"></div>
      <h3>Add a device</h3>
      <div class="muted" style="font-size:13px;line-height:19px">Scan this with Beep on the new phone or computer. Its chats and memories move to this account.</div>
      <div class="qr" style="justify-self:center;align-self:center;margin:0 auto;width:180px;height:180px">${qrSvg(0x51C3E2)}</div>
      <div class="field" style="display:flex;align-items:center;justify-content:center;height:56px;font-family:var(--mono);font-size:26px;font-weight:600;letter-spacing:3px;color:var(--m-ink)">HW3NC-8KDQ2</div>
      <div class="muted" style="font-size:12px;text-align:center">A code lasts ten minutes.</div>
      <div class="end" style="justify-content:space-between"><button class="text-btn" style="padding:0" data-itoast="Code copied">Copy code</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-go="iSettings">Done</button></div>
    </div>`,
  iSettings: () => {
    const opt = (title, detail, on, data) => `<button class="iopt${on ? " on" : ""}" ${data}><div class="grow"><b>${title}</b>${detail ? `<span>${detail}</span>` : ""}</div>${on ? `<span style="display:grid">${I.check}</span>` : ""}</button>`;
    const row = (icon, title, detail, data) => `<button class="setrow" ${data}><span class="muted" style="display:grid;width:20px">${icon}</span><div class="grow"><b>${title}</b><span style="display:flex;gap:6px;align-items:center">${detail}</span></div><span style="color:var(--m-dim);display:grid">${IC.gt}</span></button>`;
    const server = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>';
    return iBack("Settings", "iEmpty") + `<div class="body" style="padding:8px 20px 20px;gap:20px">
      ${S.i.admin ? `<div style="display:grid"><div class="label" style="margin-bottom:8px">Model (admin)</div>
        <div style="display:flex;align-items:center;gap:12px;margin-bottom:8px"><div class="grow"><b style="font-weight:500;font-size:14px">Auto</b><span class="muted" style="font-size:12px">${S.i.auto ? (S.i.agents ? "Jev picks an agent, which brings its prompt and thinking, on the model below" : "Jev picks the thinking for each prompt, on the model below") : (S.i.agents ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below")}</span></div><button class="switch" role="switch" aria-checked="${S.i.auto}" data-iact="auto"></button></div>
        ${`<div style="display:grid;gap:6px">${MODELS.map(([id, name, inp, out, text]) => opt(name, `$${inp} in, $${out} out per M tokens${text ? ", text only" : ""}`, S.i.model === id, `data-imodel="${id}"`)).join("")}</div>
        <div class="label" style="margin:12px 0 4px">Thinking</div>
        <div class="chips">${["Off", "Low", "Medium", "High"].map(l => `<button class="fchip" aria-pressed="${S.i.level === l}" data-ilevel="${l}">${l}</button>`).join("")}</div>`}
        <div class="muted" style="font-size:12px;margin-top:6px">${S.i.agents ? "Applies to your own turns." : "Applies to your own turns. Everyone else stays on the default."}</div></div>` : ""}
      <div style="display:grid">
        <div class="hr" style="margin:0"></div>
        ${row(I.key, "sample-account", "Kept in your iPhone backups · tap to save it to a file", 'data-itoast="Share sheet: beep-key.json"')}
        <div class="hr" style="margin:0"></div>
        ${row(server, "Server", `<span class="dot ${S.i.online ? "" : "off"}"></span>${S.i.online ? "Connected" : "Connecting"}`, "")}
        <div class="hr" style="margin:0"></div>
        ${S.i.admin ? `${row(server, "Machines", "Where Beep runs commands and coding agents", 'data-igo="iMachines"')}<div class="hr" style="margin:0"></div>` : ""}
      </div>
      ${iComputer()}
    </div>`;
  },
  iMemories: () => {
    const groups = [["People", [["Sam Rivera is a close friend, usually lunch on Saturdays", "Learned · 2 Oct"], ["Alex is the landlord", "You told Beep · 28 Sep"]]], ["Preference", [["Prefers short, practical answers", "You told Beep · Today"], ["Walks along the river when the weather is dry", "Learned · 21 Sep"]]], ["Work", [["Design review every Thursday at 10:30", "From your email · 10 Sep"]]],
      ["Setup", [["Has the Android SDK and one emulator; builds go in /srv/project/out", "Learned · Today", "m"], ["Pushes as the work account, never the personal one", "You told Beep · 30 Sep", "a"], ["Runs the tests with just test, not plain cargo test", "Learned · 3 Oct", "p"]]]];
    const empty = S.i.mem === "empty", ms = S.i.mscope || "all";
    const keep = sc => ms === "all" || (ms === "you" ? !sc : sc === ms);
    const shownGroups = groups.map(([g, list]) => [g, list.filter(x => keep(x[2]))]).filter(([, l]) => l.length);
    const clear = empty ? "" : `<button class="text-btn" style="color:var(--m-error);font-size:13px" data-iset="mem:forget">Clear all</button>`;
    let out = iBack("Memories", "iChats", clear);
    out += `<div style="display:flex;align-items:center;gap:12px;padding:8px 20px"><div class="field" style="display:flex;align-items:center;gap:8px;color:var(--m-dim);font-size:13px;flex:1">${I.search}Search memories</div><span class="muted" style="font-size:12px">${empty ? 0 : shownGroups.reduce((n, [, l]) => n + l.length, 0)}</span></div>`;
    if (!empty) out += `<div style="padding:0 20px">${mScopeChips(groups.map(([g, l]) => [g, l.map(([t, d, sc]) => [t, d, 0, sc])]), "data-iset", "mscope")}</div>`;
    out += empty ? `<div class="empty" style="font-size:13px;line-height:20px;padding:40px">Nothing remembered yet.<br>Beep learns as you chat, or tell it "remember that..."</div>`
      : `<div class="body" style="gap:6px;padding-top:0">${shownGroups.map(([g, list]) => `<div style="font-size:12px;font-weight:600;color:var(--m-dim);padding:10px 4px 2px">${g}</div>` + list.map(([t, d, scope]) => `<div class="card row" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius);cursor:pointer" data-iset="mem:edit">
        <div class="grow"><span style="font-size:13px;line-height:19px">${t}</span>${scope ? mScopeTag(scope) : ""}<span style="font-size:11px;color:var(--m-dim)">${d}</span></div>
        <button class="ib" style="width:36px;height:36px;color:var(--m-dim)" data-itoast="Forgotten" aria-label="Forget">${I.x}</button></div>`).join("")).join("")}</div>`;
    if (S.i.mem === "edit") out += `<div class="dialog-wrap"><div class="dialog"><h3>Edit memory</h3>
      <textarea class="field" rows="3">Prefers short, practical answers</textarea>
      <div class="end"><button class="text-btn" data-iset="mem:list">Cancel</button><button class="go" data-iset="mem:list" data-itoast="Saved">Save</button></div></div></div>`;
    if (S.i.mem === "forget") out += `<div class="dialog-wrap"><div class="dialog"><h3>Forget everything?</h3>
      <div class="muted">Beep will forget all 5 memories. This cannot be undone.</div>
      <div class="end"><button class="text-btn" data-iset="mem:list">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-iset="mem:empty">Forget all</button></div></div></div>`;
    return out;
  },
  iRoutines: () => {
    const rows = [["Morning briefing", "Give me a brief morning update and what's coming up today.", "Every day at 08:00", "Next: tomorrow 08:00"], ["Inbox sweep", "Check Fastmail for anything that needs a reply today.", "Weekdays at 17:30", "Next: today 17:30"], ["Water the plants", "Remind me to water the plants.", "Mon, Thu at 19:00", "Next: Thu 19:00"], ["Renew passport", "Remind me to book the passport appointment.", "Once, 14 Oct 10:00", "Next: 14 Oct 10:00"]];
    const add = `<button class="ib chunky coral" style="width:42px;height:42px" data-igo="iNewRoutine" aria-label="New routine">${I.plus}</button>`;
    return iBack("Routines", "iChats", add) + `<div class="body" style="gap:8px">
      ${rows.map(([l, p, s, n], i) => `<div class="swipe${S.i.swipe && i === 0 ? ` to-${S.i.swipe} armed` : ""}" style="border-radius:var(--radius)">
        <div class="under del">Delete${I.trash}</div><div class="under run">${I.play}Run now</div>
        <div class="card row" style="border-width:2px;border-radius:var(--radius)${S.i.swipe && i === 0 ? `;transform:translateX(${S.i.swipe === "del" ? -42 : 42}%)` : ""}">
        <div class="grow"><b style="font-weight:600;font-size:13px">${l}</b><span class="muted" style="font-size:12px;line-height:17px;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden">${p}</span>
          <span style="font-size:11px;color:var(--m-dim);margin-top:2px">${s}</span>
          ${S.i.on[i] ? `<span style="font-size:11px;color:var(--m-brand);font-weight:500">${n}</span>` : `<span style="font-size:11px;color:var(--m-dim)">Paused</span>`}</div>
        <button class="switch" role="switch" aria-checked="${S.i.on[i]}" data-ion="${i}"></button></div></div>`).join("")}
      <div class="hint">Swipe right to run now, left to delete</div>
    </div>`;
  },
  iNewRoutine: () => {
    const reps = ["Once", "Daily", "Weekdays", "Weekly", "Hourly"];
    return P.iRoutines() + `<div class="dialog-wrap"><div class="dialog">
      <h3>New routine</h3>
      <label class="fl">Name<input class="field" value="Weekly plan"></label>
      <div class="chips">${reps.map(r => `<button class="fchip" aria-pressed="${S.i.repeat === r}" data-irepeat="${r}">${r}</button>`).join("")}</div>
      ${S.i.repeat === "Weekly" ? `<div style="display:flex;gap:4px">${["M", "T", "W", "T", "F", "S", "S"].map((d, i) => `<button class="fchip day" aria-pressed="${[0, 4].includes(i)}">${d}</button>`).join("")}</div>` : ""}
      <button class="pill" style="align-self:flex-start">${S.i.repeat === "Once" ? "14 Oct, 18:00" : S.i.repeat === "Hourly" ? "At :00 past the hour" : "At 18:00"}</button>
      <label class="fl">What should Beep do?<textarea class="field" rows="3">Look at my calendar for the coming week and tell me what needs preparing.</textarea></label>
      <div class="end"><button class="text-btn" data-igo="iRoutines">Cancel</button><button class="go chunky" style="border-radius:var(--radius)" data-igo="iRoutines" data-itoast="Routine created">Create</button></div>
    </div></div>`;
  },
};
S.i = { online: true, run: "done", thoughts: "closed", fold: false, typing: false, approve: "ask", chats: "list", admin: true, model: "z-ai/glm-5.3-flash", level: "Low", auto: true, agents: false, mem: "list", swipe: "", on: [true, true, true, false], repeat: "Weekdays", perms: { loc: false, notif: false }, imported: false };
// The iPhone group screens are the Android ones: same bar, bubbles, sheets and
// group info. Their links are moved to the iPhone screens.
const I_GO = { drawer: "iChats", chat: "iChat", empty: "iEmpty", groupChat: "iGroup", groupInfo: "iGroupInfo", groupInvite: "iGroupInvite",
  groupNew: "iGroupNew", groupCode: "iGroupCode", groupJoin: "iGroupJoin" };
const iGroup = html => html.replace(/data-go="(\w+)"/g, (_, to) => `data-igo="${I_GO[to] || "iGroup"}"`)
  .replace(/data-toast=/g, "data-itoast=").replace(/data-act="/g, 'data-gact="').replace(/data-set="/g, 'data-gset="');
Object.assign(P, {
  iGroup: () => iGroup(R.groupChat()),
  iGroupNew: () => iGroup(R.groupNew()),
  iGroupInvite: () => iGroup(R.groupInvite()),
  iGroupCode: () => iGroup(R.groupCode()).replace("Enter or scan the code someone sent you", "Paste the link or code someone sent you").replace(">Invite code<", ">Invite link or code<").replace(/data-act="scanGroup">[\s\S]*?Scan a QR code/, `data-itoast="Pasted">${I.copy}Paste link`),
  iGroupJoin: () => iGroup(R.groupJoin()),
  iGroupInfo: () => iGroup(R.groupInfo()),
});
Object.assign(VARIANTS, {
  iWidget: [["Home Screen", () => { S.i.widget = "home"; render(); }, () => S.i.widget !== "lock"], ["Lock Screen", () => { S.i.widget = "lock"; render(); }, () => S.i.widget === "lock"]],
  iEmpty: [["Empty", () => { S.i.typing = false; render(); }, () => !S.i.typing], ["Typing", () => { S.i.typing = true; render(); }, () => S.i.typing], ["Offline", () => { S.i.online = !S.i.online; render(); }, () => !S.i.online]],
  iChat: [["Done", () => { S.i.run = "done"; render(); }, () => S.i.run === "done"], ["Running", () => { S.i.run = "running"; render(); }, () => S.i.run === "running"], ["Stopped", () => { S.i.run = "stopped"; render(); }, () => S.i.run === "stopped"], ["Table", () => { S.i.run = "table"; render(); }, () => S.i.run === "table"], ["Tools open", () => { S.i.fold = !S.i.fold; render(); }, () => S.i.fold]],
  iApprove: [["Asking", () => { S.i.approve = "ask"; render(); }, () => S.i.approve === "ask"], ["Allowed", () => { S.i.approve = "allowed"; render(); }, () => S.i.approve === "allowed"], ["Denied", () => { S.i.approve = "denied"; render(); }, () => S.i.approve === "denied"]],
  iSettings: [["Admin", () => { S.i.admin = true; S.i.agents = false; render(); }, () => S.i.admin && !S.i.agents], ["Agents on", () => { S.i.admin = true; S.i.agents = true; render(); }, () => S.i.admin && S.i.agents], ["Not admin", () => { S.i.admin = false; render(); }, () => !S.i.admin]],
  iMemories: [["List", () => { S.i.mem = "list"; render(); }, () => S.i.mem === "list"], ["Edit", () => { S.i.mem = "edit"; render(); }, () => S.i.mem === "edit"], ["Forget all", () => { S.i.mem = "forget"; render(); }, () => S.i.mem === "forget"], ["Empty", () => { S.i.mem = "empty"; render(); }, () => S.i.mem === "empty"]],
  iRoutines: [["List", () => { S.i.swipe = ""; render(); }, () => !S.i.swipe], ["Run now", () => { S.i.swipe = "run"; render(); }, () => S.i.swipe === "run"], ["Delete", () => { S.i.swipe = "del"; render(); }, () => S.i.swipe === "del"]],
  iGroupInfo: [
    ["Reminders", () => { S.gRem = "list"; render(); }, () => S.gRem === "list"],
    ["Deleted one", () => { S.gRem = "deleted"; render(); }, () => S.gRem === "deleted"],
    ["No reminders", () => { S.gRem = "empty"; render(); }, () => S.gRem === "empty"],
  ],
  iNewRoutine: [["Weekdays", () => { S.i.repeat = "Weekdays"; render(); }, () => S.i.repeat === "Weekdays"], ["Weekly", () => { S.i.repeat = "Weekly"; render(); }, () => S.i.repeat === "Weekly"], ["Once", () => { S.i.repeat = "Once"; render(); }, () => S.i.repeat === "Once"]],
});
document.getElementById("ios").addEventListener("click", e => {
  const el = e.target.closest("[data-igo],[data-iact],[data-gact],[data-gset],[data-itoast],[data-iperm],[data-iapprove],[data-iset],[data-ilevel],[data-ion],[data-irepeat],[data-imodel]");
  if (!el) return;
  const d = el.dataset;
  if (d.itoast) toast(d.itoast);
  if (d.gset) { const [k, v] = d.gset.split(":"); S[k] = v === "true" ? true : v === "false" ? false : v; if (d.igo) go(d.igo); else render(); return; }
  if (d.iperm) { S.i.perms[d.iperm] = true; render(); return; }
  if (d.gact === "gRemDel") { S.gRem = "deleted"; render(); return; }
  if (d.gact === "drawerSelect") { S.drawerMode = "select"; render(); return; }
  if (d.gact === "drawerBulk") { S.drawerMode = "bulk"; render(); return; }
  if (d.gact === "drawerDone") { S.drawerMode = "list"; render(); return; }
  if (d.gact === "shareClose") { S.drawerMode = "list"; go("iChats"); return; }
  if (d.gact === "shareStop") { S.share = "stopped"; render(); return; }
  if (d.gact === "shareApps") { S.shareApps = !S.shareApps; render(); return; }
  if (d.iapprove) { S.i.approve = d.iapprove; render(); return; }
  if (d.iset) { const [k, v] = d.iset.split(":"); S.i[k] = v; render(); return; }
  if (d.ilevel) { S.i.level = d.ilevel; render(); return; }
  if (d.imodel) { S.i.model = d.imodel; render(); return; }
  if (d.ion) { S.i.on[+d.ion] = !S.i.on[+d.ion]; render(); return; }
  if (d.irepeat) { S.i.repeat = d.irepeat; render(); return; }
  const a = d.iact;
  if (a === "import") { S.i.imported = true; toast("Key imported"); render(); return; }
  if (a === "computer") { S.i.computer = true; toast("Joined sample-account"); render(); return; }
  if (a === "showcode") { go("iShowCode"); return; }
  if (a === "widget") { S.i.typing = true; go("iEmpty"); return; }
  if (a === "stop") { S.i.run = "stopped"; if (S.screen === "iApprove") S.i.approve = "denied"; render(); return; }
  if (a === "retry") { S.i.run = "running"; render(); return; }
  if (a === "reconnect") { S.i.online = true; render(); return; }
  if (a === "fold") { S.i.fold = !S.i.fold; render(); return; }
  if (a === "thoughts") { S.i.thoughts = S.i.thoughts === "open" ? "closed" : "open"; render(); return; }
  if (a === "auto") { S.i.auto = !S.i.auto; render(); return; }
  if (d.igo) { if (d.igo === "iChats") S.drawerMode = "list"; if (d.igo === "iMemories") S.i.mem = "list"; go(d.igo); }
});
