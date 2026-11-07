
// flashmind's mark.
const BI = (n = 20) => `<svg width="${n}" height="${n}" viewBox="0 0 28 28" fill="currentColor" aria-hidden="true"><path d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z"/></svg>`;
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
const TINTS = ["var(--m-t0)", "var(--m-t1)", "var(--m-t2)"];
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
/// Which tools are on: the user's picks, or else the first 25 usable ones.
function mcpOn(tools) {
  if (S.mcpOn) return S.mcpOn;
  const on = {};
  tools.filter(t => !MCP_TOO_LARGE.includes(t)).slice(0, MCP_LIMIT).forEach(t => on[t] = true);
  return on;
}
const MCP_SERVERS = [["Team wiki", "W", "wiki.example.com", 5], ["Home Assistant", "H", "ha.example.net", 0]];

const S = {
  screen: "iEmpty",
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
};

const VARIANTS = {
  groupInfo: [
    ["Reminders", () => { S.gRem = "list"; render(); }, () => S.gRem === "list"],
    ["Deleted one", () => { S.gRem = "deleted"; render(); }, () => S.gRem === "deleted"],
    ["No reminders", () => { S.gRem = "empty"; render(); }, () => S.gRem === "empty"],
  ],
  appDetail: [
    ["Ask first", () => { S.level = "ask"; render(); }, () => S.level === "ask"],
    ["Always allow", () => { S.level = "allow"; render(); }, () => S.level === "allow"],
    ["Read only", () => { S.level = "read"; render(); }, () => S.level === "read"],
    ["Two accounts", () => { S.fm = "two"; render(); }, () => (S.fm || "two") === "two"],
    ["One account", () => { S.fm = "one"; render(); }, () => S.fm === "one"],
    ["Adding an account", () => { S.fm = "adding"; render(); }, () => S.fm === "adding"],
    ["Removing an account", () => { S.fm = "remove"; render(); }, () => S.fm === "remove"],
  ],
  drawer: [
    ["List", () => { S.drawerMode = "list"; render(); }, () => S.drawerMode === "list"],
    ["Selecting", () => { S.drawerMode = "select"; render(); }, () => S.drawerMode === "select"],
    ["Delete selected", () => { S.drawerMode = "bulk"; render(); }, () => S.drawerMode === "bulk"],
    ["Searching", () => { S.drawerMode = "search"; render(); }, () => S.drawerMode === "search"],
    ["More", () => { S.drawerMode = "more"; render(); }, () => S.drawerMode === "more"],
    ["Long press", () => { S.drawerMode = "menu"; render(); }, () => S.drawerMode === "menu"],
    ["Long press group", () => { S.drawerMode = "groupMenu"; render(); }, () => S.drawerMode === "groupMenu"],
  ],
  share: [
    ["Shared", () => { S.share = "shared"; render(); }, () => S.share === "shared"],
    ["Updated", () => { S.share = "updated"; render(); }, () => S.share === "updated"],
    ["Stopped", () => { S.share = "stopped"; render(); }, () => S.share === "stopped"],
  ],
  shared: [
    ["Loaded", () => { S.shared = "loaded"; render(); }, () => S.shared === "loaded"],
    ["Loading", () => { S.shared = "loading"; render(); }, () => S.shared === "loading"],
    ["Gone", () => { S.shared = "gone"; render(); }, () => S.shared === "gone"],
  ],
  apps: [
    ["With key", () => { S.apps = "key"; S.mcp = "list"; render(); }, () => S.apps === "key" && S.mcp === "list"],
    ["No servers added", () => { S.apps = "key"; S.mcp = "none"; render(); }, () => S.mcp === "none"],
    ["No key", () => { S.apps = "nokey"; render(); }, () => S.apps === "nokey"],
  ],
  mcpAdd: [
    ["Empty", () => { S.mcpAdd = "form"; render(); }, () => S.mcpAdd === "form"],
    ["Connecting", () => { S.mcpAdd = "connecting"; render(); }, () => S.mcpAdd === "connecting"],
    ["Signing in", () => { S.mcpAdd = "signin"; render(); }, () => S.mcpAdd === "signin"],
    ["Unreachable", () => { S.mcpAdd = "error"; render(); }, () => S.mcpAdd === "error"],
  ],
  mcpDetail: [
    ["Connected", () => { S.mcpDetail = "ok"; S.mcpOn = null; S.mcpAsk = null; render(); }, () => S.mcpDetail === "ok"],
    ["Many tools", () => { S.mcpDetail = "many"; S.mcpOn = null; S.mcpAsk = null; render(); }, () => S.mcpDetail === "many" && !S.mcpAsk],
    ["Over 25", () => { S.mcpDetail = "many"; S.mcpOn = null; S.mcpAsk = "Search people"; render(); }, () => S.mcpDetail === "many" && !!S.mcpAsk],
    ["Sign-in expired", () => { S.mcpDetail = "signin"; render(); }, () => S.mcpDetail === "signin"],
  ],
};
const MODELS = [
  ["z-ai/glm-5.3-flash", "GLM 5.3 Flash (default)", "0.15", "0.50", false],
  ["openai/gpt-6-luna", "GPT-6 Luna", "0.10", "0.50", false],
  ["deepseek/deepseek-v4.1-flash", "DeepSeek V4.1 Flash", "0.30", "1.20", false],
  ["google/gemini-3.8-flash", "Gemini 3.8 Flash", "0.75", "3.75", false],
];

const esc = s => String(s).replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));

function topBar() {
  return `<div class="bar">
    <button class="ib chunky" style="width:42px;height:42px" data-go="drawer" aria-label="Conversations">${I.menu}</button>
    <div style="flex:1;display:grid;gap:3px">
      <div style="font-weight:600;font-size:16px;line-height:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${S.screen === "empty" ? "New conversation" : "Boiler and Saturday weather"}</div>
      <div class="sub"><span class="dot ${S.connected ? "" : "off"}"></span>${S.connected ? "Connected" : '<span data-act="reconnect" style="cursor:pointer">Tap to reconnect</span>'}</div>
    </div>
    <button class="ib muted" data-go="empty" aria-label="New chat">${I.plus}</button>
    ${S.floating ? "" : `<button class="ib muted" data-go="bubble" aria-label="Float chat">${I.bubble}</button>`}
  </div>`;
}
function composer(ph = "Message") {
  return `<div class="composer"><div class="input" style="padding-right:6px"><span style="flex:1">${ph}</span><button class="ib" style="width:36px;height:36px;color:var(--m-muted)" data-go="camera" data-set="cam:sheet" aria-label="Add a photo">${IC.image}</button></div><button class="round coral" aria-label="Talk">${I.mic}</button></div>`;
}
function back(title, extra = "") {
  return `<div class="bar" style="padding-left:6px"><button class="ib" data-go="chat" aria-label="Back">${I.back}</button><div class="title" style="flex:1">${title}</div>${extra}</div>`;
}
