
const I = {
  menu: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h10"/></svg>',
  back: '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5M11 6l-6 6 6 6"/></svg>',
  plus: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg>',
  mic: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="9" y="3" width="6" height="12" rx="3"/><path d="M5 11a7 7 0 0014 0M12 18v3"/></svg>',
  bubble: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="8" cy="15" r="5"/><circle cx="17" cy="7" r="3"/><circle cx="18" cy="17" r="2"/></svg>',
  trash: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/></svg>',
  edit: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20h4L19 9l-4-4L4 16v4z"/></svg>',
  brain: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 4a3 3 0 00-3 3 3 3 0 00-2 5 3 3 0 002 5 3 3 0 006 0V4a3 3 0 00-3 0zM15 4a3 3 0 013 3 3 3 0 012 5 3 3 0 01-2 5 3 3 0 01-6 0"/></svg>',
  speaker: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9v6h4l5 4V5L8 9H4z"/><path d="M16.5 8.5a5 5 0 010 7M19 6a8.5 8.5 0 010 12"/></svg>',
  clock: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>',
  puzzle: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M10 4h4v3a2 2 0 104 0V4h2v6h-3a2 2 0 100 4h3v6h-6v-3a2 2 0 10-4 0v3H4v-6h3a2 2 0 100-4H4V4h6z"/></svg>',
  gear: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M5 19l2-2M17 7l2-2"/></svg>',
  search: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg>',
  check: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-10"/></svg>',
  x: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  chev: '<svg class="chev" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
  pushpin: '<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  qrcode: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><path d="M14 14h3v3h-3zM20 14v1M14 20h1M17 20h4v-3"/></svg>',
  exit: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M15 4h4v16h-4M10 8l-4 4 4 4M6 12h11"/></svg>',
  pin: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 21s-7-6-7-11a7 7 0 0114 0c0 5-7 11-7 11z"/><circle cx="12" cy="10" r="2.5"/></svg>',
  bell: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 16V11a6 6 0 0112 0v5l2 2H4l2-2zM10 21h4"/></svg>',
  link: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M10 14a4 4 0 006 0l3-3a4 4 0 00-6-6l-1 1M14 10a4 4 0 00-6 0l-3 3a4 4 0 006 6l1-1"/></svg>',
  key: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M17 6l3 3"/></svg>',
  apps: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="4" width="6" height="6" rx="1"/><rect x="14" y="4" width="6" height="6" rx="1"/><rect x="4" y="14" width="6" height="6" rx="1"/><rect x="14" y="14" width="6" height="6" rx="1"/></svg>',
  expand: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7"/></svg>',
  dots: '<svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="5" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="12" cy="19" r="2"/></svg>',
  copy: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/></svg>',
  quote: '<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M5 18h5v-6H7c0-2 1-3 3-3V6c-3 0-5 2-5 6v6zm9 0h5v-6h-3c0-2 1-3 3-3V6c-3 0-5 2-5 6v6z"/></svg>',
  play: '<svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>',
  dir: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 2l10 10-10 10L2 12z"/><path d="M9 14v-3h5M12 8.5l2.5 2.5L12 13.5" stroke-linecap="round"/></svg>',
  map: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6l6-2 6 2 6-2v14l-6 2-6-2-6 2zM9 4v14M15 6v14"/></svg>',
  car: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 16V11l2-5h10l2 5v5M3 16h18v3H3zM7 19v1M17 19v1M5 11h14"/></svg>',
  shield: '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"/></svg>',
  share: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><path d="M8.6 13.5l6.8 4M15.4 6.5l-6.8 4"/></svg>',
  laptop: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="4" y="5" width="16" height="11" rx="1.5"/><path d="M2 19h20" stroke-linecap="round"/></svg>',
  phone: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18h2"/></svg>',
};
I.group = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0113 0"/><circle cx="17" cy="9" r="2.5"/><path d="M16 14a5 5 0 016 5"/></svg>';
I.send = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 12l16-8-6 16-3-7z"/></svg>';

// Updates the triage agent kept for later: letter, app, summary, when.
const UPDATES = [
  ["G", "Gmail", "TestFlight: Beep 0.1.0 (899) is ready to test.", "09:52"],
  ["C", "Calendar", "Dentist moved to Thursday at 10:00.", "08:30"],
  ["F", "Fastmail", "Renfe refunded €23.40 for the cancelled Girona train.", "Yesterday"],
  ["G", "Gmail", "Your Neon invoice for October is €19.00.", "Yesterday"],
  ["S", "Slack", "Sam moved the launch review to Friday.", "Mon"],
];
// Fastmail's own mark (from fastmail.com), which apps show instead of a letter.
const FM_LOGO = `<svg viewBox="0 0 40 40" width="70%" height="70%" aria-hidden="true"><circle cx="20" cy="20" r="16" fill="#fff"/><path d="M19.9998 19.9995L11.6665 14.2695V25.7279L16.8748 24.1645L19.9998 19.9995Z" fill="#FFC107"/><path d="M11.6665 25.7298H27.2915C27.8665 25.7298 28.3332 25.2632 28.3332 24.6882V14.2715L11.6665 25.7298Z" fill="#1b1e20"/><path d="M32.8707 11.151C34.6034 13.6674 35.6234 16.7129 35.6234 20.0001C35.6234 28.6292 28.6271 35.6256 19.998 35.6256C14.6562 35.6256 9.94344 32.9401 7.12707 28.8492L4.37435 28.7492L3.52344 31.3274C7.13071 36.5638 13.1616 40.0001 19.9998 40.0001C31.0453 40.0001 39.9998 31.0456 39.9998 20.0001C39.9998 15.7929 38.6943 11.8947 36.4762 8.67285L33.7507 8.74921L32.8707 11.151Z" fill="#69B3E7"/><path d="M4.37455 20C4.37455 11.3709 11.3709 4.37455 20 4.37455C25.3418 4.37455 30.0545 7.06 32.8709 11.1509L36.4745 8.67273C32.8691 3.43636 26.8382 0 20 0C8.95455 0 0 8.95455 0 20C0 24.2073 1.30545 28.1055 3.52364 31.3273L7.12727 28.8491C5.39455 26.3327 4.37455 23.2873 4.37455 20Z" fill="#0067B9"/></svg>`;
// Clicking an update opens a chat whose first message is Beep's, telling it to you; you reply below.
const UPDATE_CHAT = ["Calendar", "08:30", "The clinic moved your dentist appointment from Tuesday to <b>Thursday at 10:00</b>. You have a call with Sam then. Want me to ask the clinic for another time?"];
// Remote MCP servers the user added by URL: name, letter, host, tool count (0 = needs sign-in).
// A server's tools as it lists them; a schema over 8 KB leaves the tool unusable. Beep turns on the first 25 usable ones.
const MCP_FEW = ["Search pages", "Read page", "Create page", "Update page", "Add comment"];
const MCP_MANY = ["Search pages", "Read page", "Create page", "Update page", "Delete page", "Move page", "Add comment", "Resolve comment",
  "List spaces", "Create space", "Bulk export", "Archive space", "List members", "Invite member", "Remove member", "Upload file", "Read file",
  "Delete file", "List tags", "Tag page", "Untag page", "Page history", "Restore version", "Export page", "Import page", "Watch page",
  "Unwatch page", "List templates", "Create from template", "Search people", "Read analytics", "Set permissions"];
const MCP_TOO_LARGE = ["Bulk export"];
const MCP_LIMIT = 25;
const MCP_SERVERS = [["Team wiki", "W", "wiki.example.com", 5], ["Home Assistant", "H", "ha.example.net", 0]];
// Settings, This Mac: the switch, where the link stands, and the two permissions desktop control needs.
function dThisMac() {
  const m = S.d.mac || "off", on = m !== "off";
  const sub = { off: "Beep can see the screen, click, type and run commands here when you ask",
    connecting: "Connecting…",
    failed: "Couldn't connect: the server did not answer",
    perm: "Connected. Allow both below",
    ready: "Connected" }[m];
  const perm = (name, why, ok) => `<div class="dtoggle"><span><b>${name}</b><small>${why}</small></span>${ok
    ? '<span class="dtag ok">Allowed</span>'
    : `<button class="dbtn" data-toast="Opens System Settings, Privacy and Security, ${name}">Allow…</button>`}</div>`;
  const rows = on ? perm("Screen Recording", "To see the screen", m === "ready") + perm("Accessibility", "To click, type and read app controls", m === "ready") : "";
  return `<div class="dsec flat" style="padding-top:6px">This Mac</div>
        <div class="dtoggle"><span><b>Let Beep use this Mac</b><small>${sub}</small></span><button class="dswitch" role="switch" aria-checked="${on}" aria-label="Let Beep use this Mac" data-dact="mac"></button></div>${rows}`;
}

const S = {
  screen: "dEmpty",
  ended: null,
  admin: true,
  queue: "typing", queued: ["And book the plumber for Thursday morning"],
  rerun: "open", rerunModel: "openai/gpt-6-luna", rerunHeld: null, rerunThinking: null,
  model: "z-ai/glm-5.3-flash",
  thinking: "Low",
  auto: true,
  agents: true,
  foldStyle: "icons",
  summary: true,
  connected: true,
  tools: { arrived: 1, open: false, run: true, fail: true },
  starters: "templates", held: null, upd: "some",
  drawerMode: "list",
  share: "shared", shared: "loaded", images: "gallery", viewer: false, file: "ready",
  routinesEmpty: false,
  enabled: [true, true, true, false],
  deleted: null, snack: false, swipe: null,
  repeat: "Daily", days: [1, 4], hour: 9, minute: 0,
  memories: "list",
  apps: "key", mcp: "list", mcpAdd: "form", mcpDetail: "ok",
  bubbleStyle: "Beep head",
  perms: { loc: false, notif: false }, keyImported: false, computer: "none", onb: false,
  place: "directions", grabApp: true,
  menu: "prompt",
  recall: "open",
  callout: { side: "right", kind: "reply", shown: true },
  approve: "ask", always: false,
  run: "replying",
  thoughts: false,
  math: "rendered",
  level: "ask", allowed: { "Send email": false, "Reply to email": true, "Move to trash": false, "Save a draft": true },
  devices: "linked",
  group: "typing", groupNotif: false, groupAsk: false, shareApps: true, gRem: "list", invite: "link", join: "ask",
  d: { work: "ask", approve: "ask", think: false, stopped: false, recent: false, link: "waiting", chats: "list", sel: 0, t: 0, left: 272, group: "done", gnew: "new", invite: "code", shareApps: false, leave: false, file: "ready", gnotif: "message", att: "one", macf: "done", mem: "list", rt: "list", sk: "list", nr: "day", gRem: "list", set: "linked", update: false },
};

const VARIANTS = {
  dEmpty: [
    ["Empty", () => { S.d.multi = false; render(); }, () => !S.d.multi],
    ["Multiline", () => { S.d.multi = true; render(); }, () => !!S.d.multi],
    ["Not linked", () => { S.d.unlinked = !S.d.unlinked; render(); }, () => !!S.d.unlinked],
    ["First run", () => { S.d.firstRun = !S.d.firstRun; render(); }, () => !!S.d.firstRun],
  ],
  dStream: [
    ["Replay", () => { S.d.stopped = false; S.d.failed = false; S.d.sfold = ""; S.d.t = 0; render(); }, () => false],
    ["Thinking open", () => { S.d.think = !S.d.think; render(); }, () => S.d.think],
    ["Stopped", () => { S.d.stopped = !S.d.stopped; S.d.failed = false; S.d.t = S.d.stopped ? Math.max(S.d.t, 30) : 0; render(); }, () => S.d.stopped],
    ["Failed", () => { S.d.failed = !S.d.failed; S.d.stopped = false; S.d.t = S.d.failed ? Math.max(S.d.t, 30) : 0; render(); }, () => !!S.d.failed],
    ["Scrolled up", () => { S.d.up = !S.d.up; render(); }, () => !!S.d.up],
    ["Queued", () => { S.d.queued = S.d.queued ? 0 : 2; render(); }, () => !!S.d.queued],
  ],
  dDone: [
    ["Plan", () => { S.d.done = "plan"; render(); }, () => !S.d.done || S.d.done === "plan"],
    ["Math", () => { S.d.done = "math"; render(); }, () => S.d.done === "math"],
    ["Long chat", () => { S.d.done = "long"; S.d.mapTip = false; render(); }, () => S.d.done === "long" && !S.d.mapTip],
    ["Map hover", () => { S.d.done = "long"; S.d.mapTip = true; render(); }, () => S.d.done === "long" && !!S.d.mapTip],
    ["Full chat", () => { S.d.done = "full"; render(); }, () => S.d.done === "full"],
    ["Agent finished", () => { S.d.done = "agent"; render(); }, () => S.d.done === "agent"],
    ["Background work waiting", () => { S.d.done = "work"; render(); }, () => S.d.done === "work"],
  ],
  dApprove: [
    ["Waiting", () => { S.d.approve = "ask"; render(); }, () => S.d.approve === "ask"],
    ["Allowed", () => { S.d.approve = "allowed"; render(); }, () => S.d.approve === "allowed"],
    ["Always", () => { S.d.approve = "always"; render(); }, () => S.d.approve === "always"],
    ["Denied", () => { S.d.approve = "denied"; render(); }, () => S.d.approve === "denied"],
  ],
  dChats: [
    ["Recent", () => { S.d.chats = "list"; S.d.sel = 0; render(); }, () => S.d.chats === "list"],
    ["Searching", () => { S.d.chats = "search"; S.d.sel = 0; render(); }, () => S.d.chats === "search"],
    ["Actions", () => { S.d.chats = "actions"; S.d.sel = 3; render(); }, () => S.d.chats === "actions"],
    ["Group actions", () => { S.d.chats = "groupActions"; S.d.sel = 0; render(); }, () => S.d.chats === "groupActions"],
    ["Rename", () => { S.d.chats = "rename"; S.d.sel = 3; render(); }, () => S.d.chats === "rename"],
    ["Delete", () => { S.d.chats = "delete"; S.d.sel = 3; render(); }, () => S.d.chats === "delete"],
    ["Share", () => { S.d.chats = "share"; S.d.sel = 3; render(); }, () => S.d.chats === "share"],
  ],
  dFile: [
    ["Ready", () => { S.d.file = "ready"; render(); }, () => S.d.file === "ready"],
    ["Downloading", () => { S.d.file = "loading"; render(); }, () => S.d.file === "loading"],
    ["Gone", () => { S.d.file = "gone"; render(); }, () => S.d.file === "gone"],
  ],
  dGroup: [
    ["Replied", () => { S.d.group = "done"; render(); }, () => S.d.group === "done"],
    ["Beep typing", () => { S.d.group = "typing"; render(); }, () => S.d.group === "typing"],
    ["Mention", () => { S.d.group = "mention"; render(); }, () => S.d.group === "mention"],
    ["Sending", () => { S.d.group = "sending"; render(); }, () => S.d.group === "sending"],
  ],
  dGroupNew: [
    ["New", () => { S.d.gnew = "new"; render(); }, () => S.d.gnew === "new"],
    ["Code", () => { S.d.gnew = "code"; render(); }, () => S.d.gnew === "code"],
    ["Wrong code", () => { S.d.gnew = "wrong"; render(); }, () => S.d.gnew === "wrong"],
    ["Join", () => { S.d.gnew = "join"; render(); }, () => S.d.gnew === "join"],
    ["Name taken", () => { S.d.gnew = "taken"; render(); }, () => S.d.gnew === "taken"],
  ],
  dGroupInvite: [
    ["Code", () => { S.d.invite = "code"; render(); }, () => S.d.invite === "code"],
    ["Reset", () => { S.d.invite = "reset"; render(); }, () => S.d.invite === "reset"],
  ],
  dGroupInfo: [
    ["Apps off", () => { S.d.shareApps = false; S.d.leave = false; render(); }, () => !S.d.shareApps && !S.d.leave],
    ["Apps shared", () => { S.d.shareApps = true; S.d.leave = false; render(); }, () => S.d.shareApps && !S.d.leave],
    ["Leaving", () => { S.d.leave = true; render(); }, () => S.d.leave],
    ["Reminders", () => { S.d.gRem = "list"; render(); }, () => S.d.gRem === "list"],
    ["No reminders", () => { S.d.gRem = "empty"; render(); }, () => S.d.gRem === "empty"],
  ],
  dNotifGroup: [
    ["Message", () => { S.d.gnotif = "message"; render(); }, () => S.d.gnotif === "message"],
    ["Approval", () => { S.d.gnotif = "approve"; render(); }, () => S.d.gnotif === "approve"],
  ],
  dTray: [
    ["Menu", () => { S.d.recent = false; S.d.update = false; render(); }, () => !S.d.recent && !S.d.update],
    ["Recent open", () => { S.d.recent = true; render(); }, () => S.d.recent && !S.d.update],
    ["Update ready", () => { S.d.recent = false; S.d.update = true; render(); }, () => S.d.update],
  ],
  dSettings: [
    ["Own account", () => { S.d.set = "owner"; render(); }, () => S.d.set === "owner"],
    ["Linked", () => { S.d.set = "linked"; render(); }, () => S.d.set === "linked"],
    ["Unlinking", () => { S.d.set = "unlink"; render(); }, () => S.d.set === "unlink"],
    ["Not linked", () => { S.d.set = "none"; render(); }, () => S.d.set === "none"],
    ["Beep server, signing out", () => { S.d.set = "signout"; render(); }, () => S.d.set === "signout"],
    ["Beep server, signed out", () => { S.d.set = "out"; render(); }, () => S.d.set === "out"],
    ["Beep server, not reachable", () => { S.d.set = "down"; render(); }, () => S.d.set === "down"],
    ["Mac off", () => { S.d.mac = "off"; if (S.d.set === "none") S.d.set = "linked"; render(); }, () => (S.d.mac || "off") === "off" && S.d.set !== "none"],
    ["Mac needs permissions", () => { S.d.mac = "perm"; if (S.d.set === "none") S.d.set = "linked"; render(); }, () => S.d.mac === "perm"],
    ["Mac ready", () => { S.d.mac = "ready"; if (S.d.set === "none") S.d.set = "linked"; render(); }, () => S.d.mac === "ready"],
    ["Mac connecting", () => { S.d.mac = "connecting"; if (S.d.set === "none") S.d.set = "linked"; render(); }, () => S.d.mac === "connecting"],
    ["Mac failed", () => { S.d.mac = "failed"; if (S.d.set === "none") S.d.set = "linked"; render(); }, () => S.d.mac === "failed"],
  ],
  dLink: [
    ["Sign in", () => { S.d.link = "signin"; render(); }, () => S.d.link === "signin"],
    ["Waiting for the browser", () => { S.d.link = "browser"; render(); }, () => S.d.link === "browser"],
    ["Sign-in failed", () => { S.d.link = "signfail"; render(); }, () => S.d.link === "signfail"],
    ["Own server", () => { S.d.link = "ownserver"; render(); }, () => S.d.link === "ownserver"],
    ["Waiting", () => { S.d.link = "waiting"; S.d.left = 272; render(); }, () => S.d.link === "waiting"],
    ["Join code", () => { S.d.link = "join"; render(); }, () => S.d.link === "join"],
    ["Linked", () => { S.d.link = "linked"; render(); }, () => S.d.link === "linked"],
    ["Expired", () => { S.d.link = "expired"; render(); }, () => S.d.link === "expired"],
  ],
  dAttach: [
    ["One image", () => { S.d.att = "one"; render(); }, () => S.d.att === "one"],
    ["Several", () => { S.d.att = "three"; render(); }, () => S.d.att === "three"],
    ["Too many", () => { S.d.att = "max"; render(); }, () => S.d.att === "max"],
    ["Files", () => { S.d.att = "files"; render(); }, () => S.d.att === "files"],
    ["File too large", () => { S.d.att = "big"; render(); }, () => S.d.att === "big"],
    ["Dropping", () => { S.d.att = "drop"; render(); }, () => S.d.att === "drop"],
    ["Sent", () => { S.d.att = "sent"; render(); }, () => S.d.att === "sent"],
    ["Sent with a file", () => { S.d.att = "sentFile"; render(); }, () => S.d.att === "sentFile"],
  ],
  dMemories: [
    ["List", () => { S.d.mem = "list"; render(); }, () => S.d.mem === "list"],
    ["Searching", () => { S.d.mem = "search"; S.d.mscope = "all"; render(); }, () => S.d.mem === "search"],
    ["Editing", () => { S.d.mem = "edit"; S.d.mscope = "all"; render(); }, () => S.d.mem === "edit"],
    ["Forget all", () => { S.d.mem = "clear"; S.d.mscope = "all"; render(); }, () => S.d.mem === "clear"],
    ["Empty", () => { S.d.mem = "empty"; S.d.mscope = "all"; render(); }, () => S.d.mem === "empty"],
  ],
  dSkills: [
    ["List", () => { S.d.sk = "list"; render(); }, () => S.d.sk === "list"],
    ["Actions", () => { S.d.sk = "actions"; render(); }, () => S.d.sk === "actions"],
    ["Skill", () => { S.d.sk = "view"; render(); }, () => S.d.sk === "view"],
    ["Editing", () => { S.d.sk = "edit"; render(); }, () => S.d.sk === "edit"],
    ["Delete from list", () => { S.d.sk = "rowdelete"; render(); }, () => S.d.sk === "rowdelete"],
    ["Delete", () => { S.d.sk = "delete"; render(); }, () => S.d.sk === "delete"],
    ["Empty", () => { S.d.sk = "empty"; render(); }, () => S.d.sk === "empty"],
  ],
  dRoutines: [
    ["List", () => { S.d.rt = "list"; render(); }, () => S.d.rt === "list"],
    ["Actions", () => { S.d.rt = "actions"; render(); }, () => S.d.rt === "actions"],
    ["Delete", () => { S.d.rt = "delete"; render(); }, () => S.d.rt === "delete"],
    ["Empty", () => { S.d.rt = "empty"; render(); }, () => S.d.rt === "empty"],
  ],
  dNewRoutine: [
    ["Every day", () => { S.d.nr = "day"; render(); }, () => S.d.nr === "day"],
    ["Weekly", () => { S.d.nr = "week"; render(); }, () => S.d.nr === "week"],
    ["Once", () => { S.d.nr = "once"; render(); }, () => S.d.nr === "once"],
    ["Cron", () => { S.d.nr = "cron"; render(); }, () => S.d.nr === "cron"],
    ["Every hour", () => { S.d.nr = "hour"; render(); }, () => S.d.nr === "hour"],
    ["Missing task", () => { S.d.nr = "err"; render(); }, () => S.d.nr === "err"],
    ["No day picked", () => { S.d.nr = "noday"; render(); }, () => S.d.nr === "noday"],
  ],
};
// The server's agents as GetAgents returns them: name, when, model and thinking.
const AGENTS = [
  ["General", "Reminders, briefings, quick facts, anything else", "Haiku 5.5, low"],
  ["Getting around", "Directions, transit, flights, hotels, visas", "Haiku 5.5, medium"],
  ["Shopping", "Finding and comparing products, where to buy", "DeepSeek V4.1 Flash, medium"],
  ["Going out", "Restaurants, bars, activities, plans", "DeepSeek V4.1 Flash, medium"],
  ["Research", "Explaining and comparing in depth", "GPT-6 Luna, high"],
  ["Inbox and admin", "Mail, Slack, bills, paperwork", "Haiku 5.5, medium"],
];

const esc = s => String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const pad = n => String(n).padStart(2, "0");

// Shared desktop action buttons use the dbtn styles and their standard variants.
function desktopButton(label, cls = "", attributes = {}, content = null) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = cls ? `dbtn ${cls}` : "dbtn";
  button.textContent = label;

  if (content !== null) button.innerHTML = content;

  for (const [name, value] of Object.entries(attributes)) {
    if (value === false || value === null) continue;
    button.setAttribute(name, value === true ? "" : String(value));
  }

  return button.outerHTML;
}
