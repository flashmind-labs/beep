// ---- Desktop (macOS tray client) ----
const DESK_SCREENS = [
  { group: "Floating bar", items: [
    ["dEmpty", "Bar, empty", "PromptBar"],
    ["dAttach", "Images and files attached", "PromptBar, attachments"],
    ["dStream", "Streaming", "PromptBar, running"],
    ["dDone", "Finished reply", "PromptBar, reply"],
    ["dApprove", "Approval", "PromptBar, approval"],
    ["dChats", "Chat list", "ChatPicker"],
    ["dFile", "File in a reply", "FileCard"],
    ["dPhone", "Phone-only notice", "PromptBar, phone tools"],
    ["dKeys", "Shortcuts", "ShortcutSheet"],
  ]},
  { group: "Group chats", items: [
    ["dGroup", "Group chat", "GroupView"],
    ["dGroupNew", "New or join", "GroupForm"],
    ["dGroupInvite", "Invite code", "InviteWindow"],
    ["dGroupInfo", "Group info", "GroupInfo"],
  ]},
  { group: "Memories and routines", items: [
    ["dMemories", "Memories", "MemoryView"],
    ["dRoutines", "Routines", "RoutineView"],
    ["dNewRoutine", "New routine", "RoutineForm"],
    ["dSkills", "Skills", "SkillView"],
  ]},
  { group: "Menu bar and windows", items: [
    ["dTray", "Tray menu", "TrayMenu"],
    ["dSettings", "Settings", "SettingsView"],
    ["dLink", "Link window", "LinkWindow"],
    ["dInvite", "Link a phone", "InviteWindow, devices"],
    ["dInput", "Input Monitoring", "HotkeyPrompt"],
  ]},
  { group: "Notifications", items: [
    ["dNotif", "Routine result", "Notification"],
    ["dNotifApprove", "Approval", "Notification, actions"],
    ["dNotifGroup", "Group message", "Notification, group"],
    ["dNotifUpdate", "Update ready", "Notification, update"],
  ]},
];
const DI = {
  tray: '<svg width="16" height="16" viewBox="0 0 28 28" fill="currentColor" aria-hidden="true"><path d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z"/></svg>',
  wifi: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M2 9a15 15 0 0120 0M5 12.5a10 10 0 0114 0M8.5 16a5 5 0 017 0"/><circle cx="12" cy="19.5" r="1.2" fill="currentColor" stroke="none"/></svg>',
  batt: '<svg width="25" height="13" viewBox="0 0 28 14" fill="none" stroke="currentColor" stroke-width="1.4"><rect x="1" y="1.5" width="23" height="11" rx="3" opacity=".6"/><rect x="3" y="3.5" width="15" height="7" rx="1.5" fill="currentColor" stroke="none"/><path d="M26 5.5v3" stroke-linecap="round" opacity=".6"/></svg>',
  ret: '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M20 5v7H5M10 7l-5 5 5 5"/></svg>',
  chevR: '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>',
  check: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-10"/></svg>',
  x: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  chev: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>',
  pin: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  chevD: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
  group: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0113 0"/><circle cx="17" cy="9" r="2.5"/><path d="M16 14a5 5 0 016 5"/></svg>',
};
const DOCK = ["#3E6C8F", "#C9A24B", "#7C5BB5", "#4F8A5B", "#B8574B", "#5A6B76", "#2F6F6A"];

function mbar(trayOn = false, menu = "", badge = false) {
  return `<div class="mbar"><b>Finder</b><span class="apps-menu"><span>File</span><span>Edit</span><span>View</span><span>Go</span><span>Window</span><span style="position:relative;cursor:default" data-dact="dset:helpm:${S.d.helpm ? '' : '1'}" role="button" aria-haspopup="menu" aria-expanded="${!!S.d.helpm}">Help${S.d.helpm ? `<div class="nmenu" role="menu" aria-label="Help" style="left:-8px;right:auto;top:20px;width:210px"><button class="it" role="menuitem" data-dact="dset:helpm:" data-dgo="dReport"><span></span><span>Report a Problem…</span><span class="k"></span></button></div>` : ""}</span></span>
    <span class="right"><span style="position:relative;display:grid">
      <button class="tray${trayOn ? " on" : ""}" data-dgo="${trayOn ? "dEmpty" : "dTray"}" aria-label="Beep menu" aria-expanded="${trayOn}">${DI.tray}</button>
      ${badge ? '<span style="position:absolute;top:2px;right:3px;width:6px;height:6px;border-radius:50%;background:var(--d-coral)"></span>' : ""}${menu}</span>
      ${DI.wifi}${DI.batt}<span class="clock">Wed 7 Oct 09:41</span></span></div>`;
}
function deskScene(content, o = {}) {
  return mbar(o.tray, o.menu, o.badge) + `<div class="ghostwin"></div><div class="ghostwin b"></div>`
    + `<div class="dock" aria-hidden="true">${DOCK.map(c => `<i style="background:linear-gradient(160deg, ${c}, color-mix(in srgb, ${c} 55%, #000))"></i>`).join("")}</div>` + content;
}
const DQUEUED = ["Also check if the 10:00 walk clashes with anything", "Then text Sam where we meet"];
const DHINTS = [["⌘N", "new agent"], ["⌘K", "chats"], ["tap ⌥", "live"], ["hold ⌥", "talk"], ["⌘/", "shortcuts"], ["esc", "to hide"]];
// The first two say the most; the rest give way when the row is narrow. The bar's chat opens in the window from here.
const dChatRow = items => items.at(-1)?.[1] === "to hide" && !String(S.screen).startsWith("dGroup");
const dHints = (items = DHINTS) => `<div class="dhints">${items.map(([k, t], i) => `<span${i >= 2 ? ' class="lo"' : ""}><kbd>${k}</kbd>${t}</span>`).join("")}${dChatRow(items) ? '<button class="win" data-dgo="dWinChat"><kbd>⌘O</kbd>open in window</button>' : ""}${S.d.unlinked ? `<button class="st" data-dgo="dLink"><span class="ddot off"></span>Not linked</button>` : ""}</div>`;
const dBar = inner => `<div class="dbar" role="dialog" aria-label="Beep">${inner}</div>`;
// The field's bottom edge is a grip: dragging it sets how tall the field stays, remembered for the bar.
const dPrompt = (text, extra = "") => `<div class="dinput dgrip"><span class="txt">${text}</span>${extra}</div>`;
const dFollow = "";
// A step says what it is doing while it runs and what it did once done, as on the phone:
// "Recalling memories" becomes "Recalled memories".
const D_PAST = { searching: "Searched", reading: "Read", checking: "Checked", looking: "Looked", fetching: "Fetched", opening: "Opened", saving: "Saved", finding: "Found",
  getting: "Got", creating: "Created", updating: "Updated", deleting: "Deleted", sending: "Sent", setting: "Set", using: "Used", remembering: "Remembered",
  loading: "Loaded", browsing: "Browsed", scheduling: "Scheduled", running: "Ran", weighing: "Weighed", recalling: "Recalled", forgetting: "Forgot", changing: "Changed",
  logging: "Logged", starting: "Started", messaging: "Messaged", waiting: "Waited", stopping: "Stopped", planning: "Planned" };
const dPast = label => { const t = label.replace(/[.\u2026\s]+$/, ""), i = t.indexOf(" "), head = i < 0 ? t : t.slice(0, i), past = D_PAST[head.toLowerCase()]; return past ? past + t.slice(head.length) : t; };
// "Running on build-box: df -h" shows the machine as a chip.
const dHost = l => l.replace(/^(Running|Ran) on (\S+): /, '$1 on <span class="dhost">$2</span> ');
const dStep = (label, st) => `<div class="dstep${st === "run" ? " run" : ""}"><span class="s">${st === "run" ? '<span class="dspin"></span>' : st === "bad" ? `<span style="color:var(--d-err);display:grid">${DI.x}</span>` : `<span class="dok">${DI.check}</span>`}</span>${dHost(st === "run" ? label : dPast(label))}</div>`;

// A Beep chat in the bar reads like a coding agent session: "You", a turn heading that
// folds the work ("Worked for 14s · 4 tool calls"), thoughts, one row per tool call, the answer.
const AG_IC = {
  read: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4M9 13h6M9 17h4"/></svg>',
  edit: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 20h4L19 9l-4-4L4 16z"/><path d="M13.5 6.5l4 4"/></svg>',
  run: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 7l5 5-5 5M12 17h7"/></svg>',
  search: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/></svg>',
  web: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.5 3 14.5 0 18M12 3c-3 3.5-3 14.5 0 18"/></svg>',
  mcp: '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3v4M15 3v4M7 7h10v4a5 5 0 0 1-10 0zM12 16v5"/></svg>',
  ok: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L20 7"/></svg>',
  bad: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>',
  chev: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>',
  next: '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M9 6l6 6-6 6"/></svg>',
};
const dKind = l => /forecast|web|Brows|Fetch/i.test(l) ? "web" : /^(Run|Ran)\b/.test(l) ? "run" : /^(Search|Recall|Find|Found|Look)/.test(l) ? "search"
  : /^(Read|Open|Check|Get|Got|Load)/.test(l) ? "read" : /^(Creat|Updat|Mov|Send|Sent|Sav|Writ|Delet|Typ|Click|Edit|Plan|Block|Set)/.test(l) ? "edit" : "mcp";
// One tool call: kind icon, verb, target in mono, status (spinner, check, or x when it failed or was stopped).
// st: "run", "bad", "stop" or done. "Ran on build-box: df -h" keeps the machine as a chip.
function dTool(label, st, kind) {
  const l = st === "run" || st === "stop" ? label.replace(/[.…\s]+$/, "") : dPast(label), k = kind || dKind(label);
  const host = l.match(/^(\S+) on (\S+): (.*)$/), app = !host && l.match(/^([^:]{1,24}): (.+)$/);
  const sp = l.indexOf(" "), verb = host ? `${host[1]} on <span class="dhost">${host[2]}</span>` : app ? app[1] : sp < 0 ? l : l.slice(0, sp);
  const tg = host ? host[3] : app ? app[2] : sp < 0 ? "" : l.slice(sp + 1);
  const mark = st === "run" ? '<span class="dspin"></span>' : st === "bad" ? AG_IC.bad : st === "stop" ? `<span style="color:var(--d-dim);display:grid">${AG_IC.bad}</span>` : AG_IC.ok;
  return `<div class="dcoderen tool row${st === "bad" ? " bad" : ""}"><span class="ki">${AG_IC[k]}</span><span class="vb">${verb}</span><code class="tt" title="${esc(tg)}">${esc(tg)}</code><span class="rm"></span><span class="mk">${mark}</span></div>`;
}
// The turn heading. With work to fold, a button whose chevron opens and folds it (act); without, plain text.
// A harness's mark on a session row, in place of its name.
const PV_MARKS = {
  claude: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="#D97757" d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/></svg>',
  codex: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="currentColor" d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z"/></svg>',
  gemini: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="#8E9CF5" d="M11.04 19.32Q12 21.51 12 24q0-2.49.93-4.68.96-2.19 2.58-3.81t3.81-2.55Q21.51 12 24 12q-2.49 0-4.68-.93a12.3 12.3 0 0 1-3.81-2.58 12.3 12.3 0 0 1-2.58-3.81Q12 2.49 12 0q0 2.49-.96 4.68-.93 2.19-2.55 3.81a12.3 12.3 0 0 1-3.81 2.58Q2.49 12 0 12q2.49 0 4.68.96 2.19.93 3.81 2.55t2.55 3.81"/></svg>',
  pi: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="currentColor" fill-rule="evenodd" d="M3 2h13v3h3v8h-3v3H9v6H3zM9 7v4h5V7zM17 18h4v4h-4z"/></svg>',
  flsh: '<svg aria-hidden="true" viewBox="0 0 28 28"><path fill="currentColor" d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z"/></svg>',
  opencode: '<svg aria-hidden="true" viewBox="96 96 320 320"><path fill="#5A5858" d="M320 224v128H192V224z"/><path fill="#F1ECEC" fill-rule="evenodd" d="M384 416H128V96h256zM320 160H192v192h128z"/></svg>',
};
const pvMark = a => `<span class="cc-ag ${a} mark" style="width:16px;height:16px;border-radius:4px;flex:none">${PV_MARKS[a]}</span>`;
const dHead = (label, o = {}) => {
  // "Worked for 8s · Searched once": the length, then what it did, dimmer. As in turnHeadHtml.
  let first = label, stats = o.stats || "";
  const at = label.indexOf(" · ");
  if (!stats && !o.id && at > 0 && !label.startsWith("Waiting")) { first = label.slice(0, at); stats = label.slice(at + 3); }
  // A running turn's clock is on its live line; its head only folds the steps.
  if (/^(Working|Waiting)/.test(first)) {
    if (!o.act && !stats) return "";
    first = stats || "Steps"; stats = "";
  }
  const inner = `<span${o.id ? ` id="${o.id}"` : ""}>${first}</span>${stats ? `<span class="stats">· ${stats}</span>` : ""}`;
  return `<div class="dcoderturn">${o.act
    ? `<button class="hd${o.open ? " open" : ""}" data-dact="${o.act}" aria-expanded="${!!o.open}" title="${o.open ? "Fold the steps" : "Show the steps"}">${inner}${AG_IC.chev}</button>`
    : `<span class="hd">${inner}</span>`}<span class="when">${o.when || "09:41"}</span></div>`;
};
const dSweep = w => [...w].map((c, i) => `<i style="--i:${i}">${c}</i>`).join("");
// A running turn's one live line, at the bottom: the word sweeps, then its clock and what it does now.
const dLive = (word, secs, o = {}) => { const lead = !word.startsWith("Waiting") && o.doing ? o.doing : word; return `<div class="dthink dlive${word.startsWith("Waiting") ? "" : " live"}"${o.id ? ` id="${o.id}"` : ""}${o.hide ? " hidden" : ""}><span class="lb">${word.startsWith("Waiting") ? word : dSweep(lead)}</span>${secs ? `<span class="since">${secs}</span>` : ""}</div>`; };
// A thought. Live: "Thinking" sweeps; with no text yet it is the label alone (the wait before
// anything arrives), with text it is open and follows the newest line. Done: "Thought for Ns", folded.
const dThink = (o = {}) => o.live && o.text == null ? dLive("Thinking", "4s", o) : o.live
  ? `<div class="dthink${o.text != null ? " open" : ""}"${o.id ? ` id="${o.id}"` : ""}${o.hide ? " hidden" : ""}><button aria-expanded="${o.text != null}"><span class="lb">Thought</span>${o.text != null ? `<span class="cv">${AG_IC.chev}</span>` : ""}</button>${o.text != null ? `<div class="bd"><div><div class="tx"><div class="kk"${o.kk ? ` id="${o.kk}"` : ""}>${o.text}</div></div></div></div>` : ""}</div>`
  : `<div class="dthink${o.open ? " open" : ""}"${o.id ? ` id="${o.id}"` : ""}${o.hide ? " hidden" : ""}><button${o.act ? ` data-dact="${o.act}"` : ""} aria-expanded="${!!o.open}"><span class="lb">Thought for ${o.secs || 4}s</span><span class="cv">${AG_IC.chev}</span></button><div class="bd"><div><div class="tx">${o.text || ""}</div></div></div></div>`;
// "You" and the prompt, as in a coding agent session; `id` lets the prompt map jump to it.
const dYou = (ask, id) => `<div class="dcoderen user"${id ? ` id="${id}"` : ""}><b>You</b><span>${ask}</span></div>`;
// The field above a finished chat.
const D_ASK = `<div class="dinput" data-dgo="dStream" style="cursor:text"><span class="caret" style="height:22px"></span><span class="txt ph" style="margin-left:-10px">Ask a follow-up…</span></div>`;
const dLog = inner => `<div class="dcoderlog">${inner}</div>`;
const dRows = (inner, hide) => `<div class="tools"${hide ? " hidden" : ""}>${inner}</div>`;
const dDur = s => s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
const dWorked = (secs, calls) => `Worked for ${dDur(secs)}${calls ? ` · ${calls} tool ${calls === 1 ? "call" : "calls"}` : ""}`;

// The streamed turn, in tenths of a second: "Thinking" alone while nothing has arrived (0 to 4),
// the thought open and following its text (4 to 16), then one tool call every 0.8s, then the answer.
const STEPS_D = ["Searching memories for tomorrow", "Reading calendar Wed 8 Oct", "Fetching forecast 10:00 to 12:00"];
const DTHINK = "Two things: tomorrow's calendar, and whether rain lands on the 10:00 walk. Calendar first, then the hourly forecast for the morning.";
const TEXT_D = "Tomorrow you have the design review at 10:30 and lunch with Sam at 13:00. Rain is likely until about 11:00, so take an umbrella for the walk or move it to after lunch, when it should be dry and around 17°C. The review room changed to the third floor this morning.";
const STREAM_MAX = Math.floor(TEXT_D.length * 0.72), STREAM_START = 42, STREAM_END = STREAM_START + Math.ceil(STREAM_MAX / 3);
const THINK_AT = 4, THOUGHT_AT = 16, stepAt = i => THOUGHT_AT + 8 * i;
let deskTimer = null;

// The stream's state at t: what the heading says, whether its work is open, each step's status.
// The screen holds the reply mid-stream; the finished turn is the Done screen.
function streamState(t = S.d.t) {
  const halted = S.d.stopped || S.d.failed;
  const calls = STEPS_D.filter((_, i) => t >= stepAt(i)).length, secs = 1 + Math.floor(t / 10);
  const label = `${halted ? "Worked" : "Working"} for ${secs}s${calls ? ` · ${calls} tool ${calls === 1 ? "call" : "calls"}` : ""}`;
  return { halted, label, open: S.d.sfold !== "fold", step: i => t >= stepAt(i + 1) ? "ok" : halted ? "stop" : "run" };
}
function syncStream() {
  const t = S.d.t, st = streamState(t), $ = id => document.getElementById(id);
  const head = $("dheadl");
  if (!head) return;
  head.textContent = st.label.replace(/^Working for [^·]*(· )?/, "") || (st.label.startsWith("Working") ? "Steps" : st.label);
  const live = !st.halted;
  $("dwait").hidden = !(live && t < THINK_AT);
  $("dthinking").hidden = !(live && t >= THINK_AT && t < THOUGHT_AT);
  $("dthought").hidden = t < THOUGHT_AT;
  const kk = $("dkk");
  if (kk && t >= THINK_AT && t < THOUGHT_AT) {
    kk.textContent = DTHINK.slice(0, Math.ceil(DTHINK.length * (t - THINK_AT + 1) / (THOUGHT_AT - THINK_AT)));
    kk.classList.toggle("ov", kk.scrollHeight > kk.clientHeight + 1);
    kk.scrollTop = kk.scrollHeight;
  }
  STEPS_D.forEach((l, i) => {
    const el = $("ds" + i);
    if (!el) return;
    el.hidden = t < stepAt(i);
    const want = st.step(i);
    if (el.dataset.s !== want) { el.dataset.s = want; el.innerHTML = dTool(l, want); }
  });
  const n = Math.max(0, Math.min(STREAM_MAX, (t - STREAM_START) * 3));
  $("dtext").textContent = TEXT_D.slice(0, n);
  $("dreply").hidden = n === 0;
}
function qrSvg(seed = 0x2F8A69) {
  const N = 25; let a = seed;
  const rnd = () => { a = a + 0x6D2B79F5 | 0; let t = Math.imul(a ^ a >>> 15, 1 | a); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; };
  const finder = (x, y) => (x < 8 && y < 8) || (x > N - 9 && y < 8) || (x < 8 && y > N - 9);
  let d = "";
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    if (finder(x, y)) continue;
    if (x >= 10 && x <= 14 && y >= 10 && y <= 14) continue;
    const on = (y === 6 || x === 6) ? (x + y) % 2 === 0 : rnd() < .5;
    if (on) d += `M${x} ${y}h1v1h-1z`;
  }
  [[0, 0], [N - 7, 0], [0, N - 7]].forEach(([x, y]) => { d += `M${x} ${y}h7v7h-7zM${x + 1} ${y + 1}v5h5v-5zM${x + 2} ${y + 2}h3v3h-3z`; });
  return `<svg viewBox="0 0 ${N} ${N}" shape-rendering="crispEdges" role="img" aria-label="Link code"><path fill="#0F1011" fill-rule="evenodd" d="${d}"/></svg>`;
}
const mmss = s => `${Math.floor(s / 60)}:${pad(s % 60)}`;

const LIVE_SAY = [["“send”", "Send now"], ["“stop”", "Send now, or stop Beep speaking"]];
const LIVE_TALK = [["tap ⌥", "Leave, or stop Beep"], ["hold ⌥", "Talk, or interrupt Beep"], ["esc", "Cancel the reply, or leave"], ["any key", "Leave and type instead"], ["⌥ ⌥", "Over the orb: bring Beep back. In Beep: leave"]];
const DKEYS = [
  ["Anywhere", [["⌥ ⌥", "Open or hide Beep"], ["⌘N", "New agent"], ["⌘K", "Chats"], ["⌘G", "New group"], ["⌘M", "Memories"], ["⌘J", "Routines, then ⌘G for grants"], ["⌘L", "Skills"], ["⌘R", "Research"], ["⌘,", "Settings"], ["⌘Q", "Quit"]]],
  ["Talking", [["tap ⌥", "Live mode, with Beep open"], ["hold ⌥", "Talk while held, with Beep open"], ...LIVE_TALK]],
  ["Say in live mode", LIVE_SAY],
  ["In a chat", [["⌘E", "Model, for admins"], [DI.ret, "Send"], [`⇧${DI.ret}`, "New line"], ["↑", "Chats, when empty"], ["⌘.", "Stop"], ["⌘R", "Research what is typed, or the offered question"], [`⌘${DI.ret}`, "Allow"], [`⇧⌘${DI.ret}`, "Allow for this session"], [`⌥⌘${DI.ret}`, "Always allow"], ["⌘D", "Deny"]]],
  ["In an agent or research", [["⌘N", "New session in the project, empty, with the start input; with a terminal on screen, a new terminal on its machine, in its folder"], ["⌘T", "New terminal filling the view, in a project; in a session, in the side pane"], ["⌘1 ⌘2 ⌘3", "Files, Changes, Artifacts in the side pane"], ["⌘O", "Open the folder in the editor"], ["⌘U", "Open or fold every thought, in a session"], ["⌘Y", "Open or fold every command, in a session"], ["⇧⌘Y", "Open or fold the last command, in a session"]]],
  ["In a list", [["↑↓", "Move"], [DI.ret, "Open"], ["tab", "Actions"], ["⌘R", "Rename"], ["⌘P", "Pin"], ["⌘⌫", "Delete or forget"], ["esc", "Back"]]],
];
const dKeyGroups = sections => `<div class="dkeys">${sections.map(([sec, rows]) => `<div class="kg"><div class="dsec">${sec}</div>${rows.map(([k, t]) => `<div class="kr"><span>${t}</span><kbd>${k}</kbd></div>`).join("")}</div>`).join("")}</div>`;
const D = {
  // Shift-Return adds a line; the field grows to six lines, then scrolls.
  dEmpty: () => deskScene(dBar(S.d.multi
    ? `<div class="dinput multi" data-dgo="dStream" style="cursor:text"><span class="txt">Draft a reply to Alex:\n- Thursday 9 to 12 works\n- ask if the plumber needs the code for the door<span class="caret" style="height:20px"></span></span></div>` + dHints([[DI.ret, "send"], [`⇧${DI.ret}`, "new line"], ["⌘K", "chats"], ["esc", "to hide"]])
    : `<div class="dinput" data-dgo="dStream" style="cursor:text"><span class="caret" style="height:22px"></span><span class="txt ph" style="margin-left:-10px">Ask Beep…</span></div>` + dHints(S.d.firstRun ? [["⌥ ⌥", "open from anywhere"], ["⌘N", "new agent"], ["⌘K", "chats"], ["esc", "to hide"]] : DHINTS))),
  dStream: () => {
    const stopped = S.d.stopped, failed = S.d.failed, halted = stopped || failed, queued = DQUEUED.slice(0, S.d.queued || 0), st = streamState();
    // Return during a reply queues the prompt; each sends as its own turn once the one before
    // finishes. A stopped or failed reply keeps them until Send. Edit puts one back in the field;
    // Up in an empty field takes the last one back to edit.
    const queue = queued.map((q, i) => `<div class="ag-queue"><span class="lbl">Queued</span><span class="t">${esc(q)}</span><span class="w">${halted ? "not sent" : "after the reply"}</span>${halted && i === 0 ? `<button data-dact="qsend">Send</button>` : ""}<button data-dact="qedit:${i}" data-toast="Back in the field to edit">Edit</button><button aria-label="Remove queued prompt ${i + 1}" data-dact="qedit:${i}" data-toast="Removed from the queue">&#215;</button></div>`).join("");
    const stop = halted ? "" : `<button class="dbtn" data-dact="stop" aria-label="Stop the reply"><span style="width:9px;height:9px;border-radius:2px;background:currentColor"></span>Stop</button>`;
    const log = dLog(`<div class="dcoderen user"><b>You</b><span>What's on tomorrow, and will it rain for the 10:00 walk? <a href="https://example.com/forecast" style="color:var(--d-emerald)">https://example.com/forecast</a></span></div>`
      + dHead(st.label, { act: "sfold", open: st.open, id: "dheadl" })
      + dRows(dThink({ live: true, id: "dwait", hide: true })
        + dThink({ live: true, id: "dthinking", text: "", kk: "dkk", hide: true })
        + dThink({ id: "dthought", secs: 4, open: S.d.think, act: "think", text: `<p>${DTHINK}</p>`, hide: true })
        + STEPS_D.map((_, i) => `<div id="ds${i}" style="display:contents" hidden></div>`).join(""), !st.open)
      + `<div class="dcoderen agent md" id="dreply" hidden><span id="dtext"></span></div>`
      + (stopped ? `<div class="dmuted">Stopped<button class="dbtn" data-dact="retry">Retry</button></div>` : "")
      + (failed ? `<div class="errc"><b>The turn did not finish</b><span>The model stopped answering after 3s. Nothing was sent or changed.</span><div class="btns"><button class="dbtn" data-dact="retry">Retry</button><button class="dbtn quiet" data-toast="Copied the error">Copy error</button></div></div>` : ""));
    return deskScene(dBar(dPrompt('<span style="color:var(--d-dim)">Ask a follow-up…</span>', stop) + `<div class="dbody">${log}</div>`
      + (S.d.up ? `<button class="dbtn dlatest" data-dact="latest"><span style="display:grid;transform:rotate(90deg)">${DI.chev}</span>Latest</button>` : "") + queue
      + dHints([[DI.ret, halted ? "send" : "queue"], ...(queued.length ? [["↑", "edit queued"]] : []), ...(halted ? [] : [["⌘.", "stop"]]), ["⌘K", "chats"], ["esc", "to hide"]])));
  },
  dDone: () => {
    if (S.d.done === "agent") return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(dYou("Fix the failing login test on build-box") + dFold(["Starting claude on build-box"], 4) + `<div class="dcoderen agent md">Claude Code started on it. I will tell you when it is done.</div><div class="dsystem">Claude Code finished</div>` + dHead("Worked for 6s", { stats: "1 tool call", act: "steps", when: "09:44" }) + `<div class="dcoderen agent md">The login test passes now on <b>fix-login</b>, and 2 files changed. The other tests still pass.</div>`)}</div>` + dHints()));
    if (S.d.done === "work") return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(dYou("Plan my Saturday around the rain") + dFold(["Fetching forecast for Saturday"], 4) + `<div class="dcoderen agent md"><b>Saturday is wet until about 11:00</b>, then dry and around 19°C.</div>`)}<div class="dworkq" style="display:grid;gap:8px;flex:none"><div class="dmuted" style="justify-content:center;font-size:11.5px;color:var(--d-dim)">Background work is waiting for you</div><div class="dcard"><div class="head"><span class="dapp">S</span><div style="flex:1;min-width:0;display:grid"><b style="font-size:13.5px">Send message with Slack?</b><span style="font-size:12px;color:var(--d-muted)">Background work asks first</span></div></div><div class="kv"><span>Channel</span><span>#launch</span><span>Text</span><span>Review moved to Friday 10:00</span></div><div class="acts"><button class="dbtn pri" data-toast="Allowed once. The work goes on.">Allow<kbd>⌘${DI.ret}</kbd></button><button class="dbtn" data-toast="Allowed for the rest of this chat. The work goes on.">Allow for this session<kbd>⇧⌘${DI.ret}</kbd></button><button class="dbtn" data-toast="Allowed, and always allowed from now on. The work goes on.">Always<kbd>⌥⌘${DI.ret}</kbd></button><button class="dbtn warn" data-toast="Denied. The work goes on without it.">Deny<kbd>⌘D</kbd></button></div></div></div></div>` + dHints([[`⌘${DI.ret}`, "allow"], [`⇧⌘${DI.ret}`, "this session"], [`⌥⌘${DI.ret}`, "always"], ["⌘D", "deny"], ["esc", "to hide"]])));
    // A long chat gets a map of its prompts on the right edge: hover a tick to read the prompt, click to jump to it.
    if (S.d.done === "long") {
      const turns = [
        ["Find a flat viewing slot with Alex next week", "Alex is free Tuesday after 17:00 and Thursday morning. Your calendar is clear on Thursday from 9 to 12, so <b>Thursday at 10:00</b> works for both of you."],
        ["What documents do I need for the rental application?", "Usually four things:<ul><li>ID or passport</li><li>The last three payslips, or a work contract</li><li>A bank statement showing the deposit</li><li>A reference from your current landlord</li></ul>"],
        ["Draft an email to the agent asking about the deposit", "Here is a draft:<div style=\"margin-top:6px;padding:8px 10px;border-radius:8px;background:var(--d-elev);font-size:13px;line-height:19px\">Hi Marta, before Thursday's viewing, could you tell me how much the deposit is and whether it can be paid in two parts? Thanks.</div>"],
        ["Is 1,450 a month fair for that area?", "It is a little above the middle. Two-bedroom flats within ten minutes of the station listed between 1,250 and 1,500 this month, so there is room to ask for 1,350."],
        ["Plan my Saturday around the rain", "<b>Saturday is wet until about 11:00</b>, then dry and around 19°C. Coffee and the pharmacy first, the river walk at 11:30, then lunch with Sam at 13:00."],
      ];
      const tops = [3, 24, 45, 70, 92], cur = 4;
      const map = `<div class="dmap" aria-label="Prompts in this chat">${turns.map(([q], i) => `<button class="tk${i === cur ? " on" : ""}${S.d.mapTip && i === 1 ? " hov" : ""}" style="top:${tops[i]}%" data-dact="jump:${i}" aria-label="${esc(q)}"><i></i><span class="tip">${esc(q)}</span></button>`).join("")}</div>`;
      const took = [6, 4, 7, 9, 12];
      return deskScene(dBar(D_ASK + `<div class="dbody" id="dlong">`
        + dLog(turns.map(([q, a], i) => dYou(esc(q), "dturn" + i) + dHead(`Worked for ${took[i]}s`, { when: `09:${pad(12 + 6 * i)}` }) + `<div class="dcoderen agent md">${a}</div>`).join(""))
        + `</div>` + map + dHints()));
    }
    // A whole chat: steps folded under each prompt, an approval with Beep's second reply after it, follow-ups under the last reply.
    if (S.d.done === "full") {
      // Each turn's work folds into its heading; the chevron opens it.
      const work = [
        [9, ["Searching memories for Saturday", "Fetching forecast for Saturday", "Reading calendar Sat 11 Oct"]],
        [11, ["Updating event Lunch with Sam", "Reading contacts Sam Rivera", "Fastmail: send_email to Sam Rivera"]],
        [3, ["Fastmail: search_email from Sam Rivera"]],
      ];
      const fold = i => { const [secs, rows] = work[i], open = S.d.fopen === String(i);
        return dHead(dWorked(secs, rows.length), { act: `dset:fopen:${open ? "" : i}`, open, when: ["09:12", "09:15", "09:38"][i] }) + (open ? dRows(rows.map(r => dTool(r)).join("")) : ""); };
      const turns = [
        ["Plan my Saturday around the rain", `${fold(0)}<div class="dcoderen agent md">
          <div><b>Saturday is wet until about 11:00</b>, then dry and around 19°C. A plan that works around it:</div>
          <ul><li><b>09:30</b> coffee and the pharmacy run, both indoors</li><li><b>11:30</b> walk along the river once it clears</li><li><b>13:00</b> lunch with Sam, already in your calendar</li></ul></div>`],
        ["Move lunch to 13:30 and tell Sam", `${fold(1)}
          <div class="dcoderen agent md">Lunch is now at 13:30 in your calendar. Here is the email to Sam:</div>
          <div class="dcard"><div class="head"><span class="dapp">F</span><div style="flex:1;min-width:0;display:grid"><b style="font-size:13.5px">Send email with Fastmail?</b><span style="font-size:12px;color:var(--d-muted)">Fastmail asks first</span></div></div>
            <div class="kv"><span>To</span><span>Sam Rivera</span><span>Subject</span><span>Lunch on Saturday</span></div>
            <div class="draft" style="margin-bottom:12px">Hi Sam, can we move lunch on Saturday to 13:30? Same place. Thanks!</div></div>
          <div class="dmuted">${I.shield.replace('width="18" height="18"', 'width="14" height="14"')}Allowed once</div>
          <div class="dcoderen agent md">Sent. Sam has it.</div>`],
        ["Did Sam answer?", `${fold(2)}
          <div class="dcoderen agent md">Yes, a few minutes ago: <i>"13:30 is perfect, see you there."</i> Nothing else changed for Saturday.</div>
          <div class="dups"><button class="dbtn" data-toast="Sent: Remind me at 13:00">Remind me at 13:00</button><button class="dbtn" data-toast="Sent: What about Sunday?">What about Sunday?</button></div>`],
      ];
      const map = `<div class="dmap" aria-label="Prompts in this chat">${turns.map(([q], i) => `<button class="tk${i === 2 ? " on" : ""}" style="top:${[3, 34, 80][i]}%" data-dact="jump:${i}" aria-label="${esc(q)}"><i></i><span class="tip">${esc(q)}</span></button>`).join("")}</div>`;
      return deskScene(dBar(D_ASK + `<div class="dbody" id="dfull">`
        + dLog(turns.map(([q, a], i) => dYou(esc(q), "dturn" + i) + a).join(""))
        + `</div>` + map + dHints()));
    }
    const math = S.d.done === "math";
    // Follow-ups sit under the finished reply; clicking one sends it as the next question.
    const ups = qs => `<div class="dups">${qs.map(q => `<button class="dbtn" data-dgo="dStream" data-toast="Sent: ${esc(q)}">${esc(q)}</button>`).join("")}</div>`;
    const steps = math ? ["Fetching web: ed25519 key generation"] : ["Searching memories for Saturday", "Fetching forecast for Saturday", "Reading calendar Sat 11 Oct", "Finding free time Saturday"];
    const thought = math ? "<p>Seed to scalar to point. Worth showing the clamping, since that is the part people ask about.</p>" : "<p>Rain timing decides the order. Indoor errands first, the walk once it clears, lunch stays where it is.</p>";
    const body = math ? `
      <div>An Ed25519 key pair comes from a 32 byte seed. Hash the seed with SHA-512, clamp the first half to get the secret scalar, then multiply the base point by it:</div>
      <math display="block"><mi>s</mi><mo>=</mo><mi>clamp</mi><mo>(</mo><mtext>SHA-512</mtext><mo>(</mo><mtext>seed</mtext><mo>)</mo><mo>[</mo><mn>0</mn><mo>..</mo><mn>32</mn><mo>]</mo><mo>)</mo><mo>,</mo><mspace width="1.2em"></mspace><mi>A</mi><mo>=</mo><mo>[</mo><mi>s</mi><mo>]</mo><mi>B</mi></math>
      <div>Clamping clears the low 3 bits and sets bit 254, so <math><mi>s</mi></math> is a multiple of the cofactor 8. The public key is <math><mi>A</mi><mo>=</mo><mo>[</mo><mi>s</mi><mo>]</mo><mi>B</mi></math> encoded as 32 bytes, and the other half of the hash becomes the nonce prefix for signing.</div>`
    : `
      <div><b>Saturday is wet until about 11:00</b>, then dry and around 19°C. A plan that works around it:</div>
      <ul><li><b>09:30</b> coffee and the pharmacy run, both indoors</li><li><b>11:30</b> walk along the river once it clears</li><li><b>13:00</b> lunch with Sam, already in your calendar</li></ul>
      <div>Want me to block the walk in your calendar?</div>`;
    // A done turn folds its work into the heading; the chevron opens the thought and the calls.
    return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(dYou(math ? "How is an Ed25519 public key made from the seed?" : "Plan my Saturday around the rain")
      + dHead(dWorked(math ? 8 : 14, steps.length), { act: "steps", open: !!S.d.steps })
      + (S.d.steps ? dRows(dThink({ secs: math ? 3 : 4, text: thought, open: !!S.d.dthk, act: `dset:dthk:${S.d.dthk ? "" : 1}` }) + steps.map(l => dTool(l)).join("")) : "")
      + `<div class="dcoderen agent md">${body}</div>` + ups(math ? ["Why clamp the scalar?", "How is a signature checked?", "Show it in Rust"] : ["Block the walk at 11:30", "What about Sunday?"]))}</div>` + dHints()));
  },
  dApprove: () => {
    const st = S.d.approve, ask = st === "ask";
    const card = ask ? `<div class="dcard">
        <div class="head"><span class="dapp">F</span><div style="flex:1;min-width:0;display:grid"><b style="font-size:13.5px">Send email with Fastmail?</b><span style="font-size:12px;color:var(--d-muted)">Fastmail asks first</span></div></div>
        <div class="kv"><span>To</span><span>Sam Rivera</span><span>Subject</span><span>Lunch on Saturday</span></div>
        <div class="draft">Hi Sam, can we move lunch on Saturday to 13:30? Same place. Thanks!</div>
        <div class="acts"><button class="dbtn pri" data-dact="allow">Allow<kbd>⌘${DI.ret}</kbd></button><button class="dbtn" data-dact="allow" title="Allow this action without asking until this chat ends">Allow for this session<kbd>⇧⌘${DI.ret}</kbd></button><button class="dbtn" data-dact="always">Always<kbd>⌥⌘${DI.ret}</kbd></button><button class="dbtn warn" data-dact="deny">Deny<kbd>⌘D</kbd></button></div>
      </div>` : `<div class="dmuted">${I.shield.replace('width="18" height="18"', 'width="14" height="14"')}${{ allowed: "Allowed once", always: "Allowed, and always allowed for Fastmail from now on", denied: "Denied" }[st]}</div>`;
    const after = { ask: "", allowed: "Sent. Sam has it.", always: "Sent. Sam has it.", denied: "Okay, I didn't send it. Want me to change the wording?" }[st];
    const steps = [["Searching email"], ["Fastmail: send email", ask ? "run" : st === "denied" ? "bad" : ""]];
    return deskScene(dBar((ask ? dAskF() : D_ASK) + `<div class="dbody">${dLog(`${dYou("Tell Sam lunch on Saturday moves to 13:30")}
      ${ask ? dWork(steps, 3) : dDone(steps, 5)}
      <div class="dcoderen agent md">Here is the email. It goes out once you allow it.</div>
      ${card}${after ? `<div class="dcoderen agent md">${after}</div>` : ""}`)}
    </div>` + dHints(ask ? [[`⌘${DI.ret}`, "allow"], [`⇧⌘${DI.ret}`, "this session"], [`⌥⌘${DI.ret}`, "always"], ["⌘D", "deny"], ["esc", "to hide"]] : undefined)), { badge: ask });
  },
  dChats: () => {
    const m = S.d.chats, search = m === "search";
    // Groups sit in the same list with a people icon. Pinned rows come first; a chat still answering shows a spinner instead of its time.
    // Tab on the selected row shows its actions inline: a chat can be renamed, pinned, shared or deleted, a group pinned, opened in info or left.
    const rows = search ? [["Train to <mark>Girona</mark>", "3 Oct", false, false, false, "The 09:12 to <mark>Girona</mark> takes 38 minutes"], ["Hotels near <mark>Girona</mark> station", "12 Aug", false, false, false, "Two are a short walk from <mark>Girona</mark> station"]]
      : [["Tokyo trip", "10:22", true, false, true, "Sam: @beep can you find us a ryokan"], ["Flat viewing questions", "29 Sep", true], ["Saturday plan", "", false, true], ["Lunch with Sam", "1 h ago"], ["Boiler and Saturday weather", "Yesterday"], ["Train to Girona", "3 Oct"]];
    const sel = m === "groupActions" ? 0 : m === "list" || search ? S.d.sel : 3;
    const acts = m === "groupActions"
      ? `<span class="acts"><button class="dbtn" data-toast="Unpinned">Unpin<kbd>⌘P</kbd></button><button class="dbtn" data-dgo="dGroupInvite">Invite</button><button class="dbtn" data-dgo="dGroupInfo">Info</button><button class="dbtn warn" data-dgo="dGroupInfo" data-dact="leaveAsk">Leave</button></span>`
      : `<span class="acts"><button class="dbtn" data-dgo="dWinChat">Open in window<kbd>⌘O</kbd></button><button class="dbtn" data-dact="chats:rename">Rename<kbd>⌘R</kbd></button><button class="dbtn" data-toast="Pinned">Pin<kbd>⌘P</kbd></button><button class="dbtn" data-dact="chats:share">Share</button><button class="dbtn warn" data-dact="chats:delete">Delete<kbd>⌘⌫</kbd></button></span>`;
    const row = ([t, w, pin, run, group, snip], i) => {
      const on = i === sel;
      if (on && m === "rename") return `<div class="dchat" aria-selected="true"><input value="Lunch with Sam" aria-label="Chat name"></div>`;
      if (on && m === "delete") return `<div class="dchat" aria-selected="true"><span class="t"><span>Delete <b>Lunch with Sam</b>?</span></span><span class="acts"><button class="dbtn warn" data-dact="chats:list" data-toast="Deleted">Delete<kbd>${DI.ret}</kbd></button><button class="dbtn quiet" data-dact="chats:list">Cancel</button></span></div>`;
      const tail = on && (m === "actions" || m === "groupActions") ? acts : run ? '<span class="run"><span class="dspin"></span>Answering</span>' : `<span class="w">${w}</span>`;
      const tag = on && (m === "actions" || m === "groupActions") ? "div" : "button";
      return `<${tag} class="dchat" role="option" aria-selected="${on}" data-dpick="${i}" data-dgroup="${group ? 1 : ""}">${pin ? `<span class="pin" aria-label="Pinned">${DI.pin}</span>` : ""}${group ? `<span class="grp" aria-label="Group">${DI.group}</span>` : ""}<span class="t"><span>${t}</span>${snip ? `<small>${snip}</small>` : ""}</span>${tail}</${tag}>`;
    };
    const share = m === "share" ? `<div class="dcard dshare"><div class="code">https://beep.example.com/s/3f9c2a7e41d84b0c9e5a6d1f08b2c7e4</div><div style="display:flex;gap:8px"><button class="dbtn pri" data-toast="Link copied">Copy link</button><button class="dbtn warn" data-dact="chats:list" data-toast="Stopped sharing. The link no longer opens.">Stop sharing</button></div><div class="dmuted" style="display:block">A copy of the chat as it is now. Only your messages and Beep's replies are shared, not tool results, thoughts or your location. Anyone with Beep can open the link.</div></div>` : "";
    const hints = m === "actions" || m === "groupActions" ? [["tab", "hide actions"], ["esc", "back"]]
      : m === "rename" ? [[DI.ret, "save"], ["esc", "cancel"]]
      : [[DI.ret, "open"], ["tab", "actions"], ["⌘N", "new agent"], ["⌘G", "new group"], ["esc", "back"]];
    return deskScene(dBar(`<div class="dinput"><span class="txt${search ? "" : " ph"}">${search ? "girona" : "Search chats and groups"}</span><kbd>⌘K</kbd></div>
      <div class="dsec">${search ? "2 results" : "Recent"}</div>
      <div class="dlist" role="listbox" aria-label="Chats">${rows.map((r, i) => row(r, i) + (i === sel ? share : "")).join("")}</div>`
      + dHints(hints)));
  },
  // send_file puts a card in the reply. The first click downloads the file to a cache, later clicks open it from there.
  dFile: () => {
    const st = S.d.file, doc = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>';
    const right = st === "loading" ? '<span class="dspin"></span>' : st === "gone" ? "" : `<span style="color:var(--d-muted);display:grid">${DI.chev}</span>`;
    return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(`${dYou("Send me the boarding pass PDF")}
      ${dDone([["Fastmail: read email"], ["Sending a file"]], 4)}
      <div class="dcoderen agent md">Here is your boarding pass for SQ012 tomorrow.</div>
      <button class="dfile${st === "gone" ? " gone" : ""}" data-toast="${st === "gone" ? "No longer available" : "Opens in Preview"}"><span class="ic">${doc}</span>
        <span style="flex:1;min-width:0;display:grid"><span class="nm">BoardingPass.pdf</span><span class="sz">${st === "gone" ? "No longer available" : st === "loading" ? "Downloading…" : "PDF, 26 KB"}</span></span>${right}</button>
      <div class="dcoderen agent md">Gate closes 40 minutes before departure.</div>`)}
    </div>` + dHints()));
  },
  dPhone: () => deskScene(dBar(D_ASK + `<div class="dbody">${dYou("Set an alarm for 6:45 and text Sam that I'm running late")}
      ${dFold(["Reading calendar"])}
      <div class="dcoderen agent md">I can't set the alarm or text Sam from here. Your first meeting tomorrow is the design review at 10:30, so 6:45 leaves plenty of time.</div>
      <div class="dmuted">${I.phone.replace('width="20" height="20"', 'width="14" height="14"')}Phone actions, like contacts and alarms, are not available on the computer. Ask from your phone.</div>
    </div>` + dFollow + dHints())),
  // The sheet covers whatever the bar showed; esc or Cmd-/ closes it.
  dKeys: () => deskScene(dBar(`<div class="dover" id="keys" role="dialog" aria-label="Keyboard shortcuts"><div class="vt"><span class="tt"><b>Keyboard shortcuts</b></span></div>${dKeyGroups(DKEYS)}${dHints([["esc", "close"]])}</div>`)),
  dTray: () => {
    const it = (label, key = "", attrs = "", check = false, hl = false) => `<button class="it${hl ? " hl" : ""}" role="menuitem" ${attrs}><span style="display:grid">${check ? DI.check : ""}</span><span>${label}</span><span class="k">${key}</span></button>`;
    const recent = S.d.recent ? `<div class="nmenu nsub" role="menu" style="top:120px">
      ${[["Saturday plan", "2 min ago"], ["Lunch with Sam", "1 h ago"], ["Boiler and Saturday weather", "Yesterday"], ["Train to Girona", "3 Oct"], ["Flat viewing questions", "29 Sep"]].map(([t, w]) => it(t, w, `data-dgo="dDone"`)).join("")}</div>` : "";
    const menu = `<div class="nmenu" role="menu" aria-label="Beep">
      ${it("Open Beep", "", `data-dgo="dHome"`)}
      ${it("Quick chat", "", `data-dgo="dEmpty"`)}
      <div class="hdr">Or press Option twice, anywhere</div>
      <hr>
      <div class="stat"><span style="display:flex;gap:7px;align-items:center"><span class="ddot"></span>Linked to Pixel</span><small>Signed in as alex</small></div>
      <hr>
      ${it("New chat", "⌘N", `data-dgo="dEmpty"`)}
      ${it("Recent", DI.chevR, `data-dact="recent" aria-haspopup="menu" aria-expanded="${S.d.recent}"`, false, S.d.recent)}
      ${it("Memories", "⌘M", `data-dgo="dMemories"`)}
      ${it("Routines", "⌘J", `data-dgo="dRoutines"`)}
      ${it("Skills", "⌘L", `data-dgo="dSkills"`)}
      ${it("Research", "⌘R", `data-dgo="dResearches"`)}
      <hr>
      <div class="hdr">Waiting</div>
      ${it(`<span style="display:flex;gap:7px;align-items:center"><span class="ddot" style="background:var(--d-coral)"></span>Allow Fastmail: send email?</span>`, "", `data-dgo="dApprove"`)}
      <hr>
      ${it("Report a Problem", "", `data-dgo="dReport"`)}
      ${it("Settings", "⌘,", `data-dgo="dSettings"`)}
      ${it("Enable Option-Option", "", `data-dgo="dInput"`)}
      ${S.d.update ? it("Restart to Update Beep", "", `title="Install Beep 0.3.0" data-toast="Restarting into Beep 0.3.0"`) : it("Check for Updates", "", `data-toast="Beep is up to date"`)}
      ${it("Show Logs", "", `data-toast="Opens the log folder in Finder"`)}
      <hr>
      ${it("Quit Beep", "⌘Q", `data-toast="Quit installs a downloaded update; disabled in the mock"`)}
      ${recent}
    </div>`;
    return deskScene("", { tray: true, menu, badge: true });
  },
  // Settings: what used to be loose items in the tray menu. Unlinking asks first, inline.
  // This Mac: off by default. On, Beep uses the Mac as a machine: its screen, mouse, keyboard and a shell.
  // macOS asks for Screen Recording and Accessibility; each row opens its page in System Settings.
  dSettings: () => {
    const st = S.d.set, on = S.d.login !== false;
    const ic = icon => `<span class="ic">${icon.replace('width="20" height="20"', 'width="15" height="15"')}</span>`;
    const row = (icon, name, tags, sub, x) => `<div class="ddev">${ic(icon)}<span class="nm">${name}${tags.map(t => `<span class="dtag${t === "Owner" ? " own" : ""}">${t}</span>`).join("")}<small>${sub}</small></span>${x ? `<button class="dx" aria-label="Remove ${x}" data-toast="${x} removed">${DI.x}</button>` : ""}</div>`;
    const link = st === "none"
      ? `<div class="dtoggle"><span><b>Not linked</b><small>Link to see your chats and memories here</small></span><button class="dbtn pri" data-dgo="dLink">Link to phone…</button></div>`
      : st === "owner"
      ? `<div class="ddevs">${row(I.laptop, "Alex's MacBook Air", ["This Mac", "Owner"], "macOS. Active now", "")}${row(I.phone, "Pixel", [], "Android. Active 5 min ago", "Pixel")}</div>
        <div><button class="dbtn" data-dgo="dInvite">Link a phone…</button></div>`
      : `<div class="ddevs">${row(I.phone, "Pixel", ["Owner"], "Android. Active 5 min ago", "")}${row(I.laptop, "Alex's MacBook Air", ["This Mac"], "macOS. Active now", "")}</div>
        ${st === "unlink" ? "" : '<div style="display:flex;gap:8px"><button class="dbtn" data-dgo="dInvite">Link a phone…</button><button class="dbtn warn" data-dact="set:unlink">Unlink this Mac</button></div>'}`;
    const confirm = st === "unlink" ? `<div class="dcard"><div style="padding:10px 12px;display:grid;gap:8px">
        <b style="font-size:13.5px">Unlink this Mac?</b><div class="dmuted" style="display:block">This Mac stops seeing your chats and memories. Your phone keeps them. To use Beep here again, start over and link it from the phone.</div>
        <div style="display:flex;gap:8px"><button class="dbtn warn" data-dact="set:none" data-toast="Shows This Mac was removed, with Start over">Unlink</button><button class="dbtn quiet" data-dact="set:linked">Cancel</button></div></div></div>` : "";
    return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS 0.2.0</small></span></div>
      <div class="dform" style="padding-top:12px">
        <div class="dsec" style="padding:0">Devices</div>${link}${confirm}
        ${st === "none" ? "" : `<div class="dsec" style="padding:6px 0 0">Apps</div>
        <div class="dtoggle"><span><b>Connected apps</b><small>2 connected: Gmail, Google Calendar</small></span><button class="dbtn" data-dgo="dApps">Manage…</button></div>`}
        ${st === "none" ? "" : dThisMac()}
        ${st === "none" ? "" : `<div class="dtoggle"><span><b>Other machines</b><small>2 linked: build-box, gpu-box</small></span><button class="dbtn" data-dgo="dMachines">Show…</button></div>`}
        <div class="dsec" style="padding:6px 0 0">General</div>
        <div class="dtoggle"><span><b>Voice notes</b><small>Speak new notices when you are at this Mac and not on a call, then keep listening. Tap Option three times to talk live.</small></span><button class="dswitch" role="switch" aria-checked="${S.d.voice === "on"}" aria-label="Voice notes" data-dact="dset:voice:${S.d.voice === "on" ? "off" : "on"}"></button></div>
        <div class="dtoggle"><span><b>When I stop talking</b><small>${S.d.vsend === "word" ? "Pauses never send. Say “send” or “stop” when you are done." : "Sends after a short pause. Saying “send” sends at once."}</small></span></div>
        <div class="dchips" role="radiogroup" aria-label="When I stop talking" style="margin-top:-6px"><button class="dchip" role="radio" aria-pressed="${S.d.vsend !== "word"}" aria-checked="${S.d.vsend !== "word"}" data-dact="dset:vsend:pause">Send when I stop talking</button><button class="dchip" role="radio" aria-pressed="${S.d.vsend === "word"}" aria-checked="${S.d.vsend === "word"}" data-dact="dset:vsend:word">Wait until I say “send”</button></div>
        <div class="dtoggle"><span><b>Launch at login</b><small>Keep Beep in the menu bar after a restart</small></span><button class="dswitch" role="switch" aria-checked="${on}" data-dact="login"></button></div>
      </div>` + dHints([["esc", "close"]])));
  },
  dLink: () => {
    const st = S.d.link;
    const qr = `<div class="qr${st === "waiting" || st === "join" ? "" : " dim"}" style="width:156px;height:156px">${qrSvg()}${st === "linked" ? `<div class="over"><span style="display:grid;justify-items:center;gap:6px"><span style="width:44px;height:44px;border-radius:50%;background:var(--d-emerald);color:#fff;display:grid;place-items:center">${I.check}</span>Linked</span></div>` : st === "expired" ? `<div class="over">Code expired</div>` : ""}</div>`;
    const own = `<div class="dor"><span>or</span></div>
        <button class="dbtn pri" data-dact="ownMac">Use Beep on this Mac</button>
        <p style="font-size:12px;margin-top:-6px;max-width:none">Starts an account on this Mac. You can link your phone to it later.</p>
        <button class="dbtn quiet" data-dact="joinOpen">Have a join code?</button>`;
    const head = `<h4>Link to your phone</h4><p>Scan with Beep on your phone: Settings, Linked devices.</p>${qr}`;
    const signing = ["signin", "browser", "signfail", "ownserver"].includes(st);
    const ownLink = `<button class="dbtn quiet" data-dact="dset:link:ownserver">Use my own server</button>`;
    const body = {
      signin: `<h4>Sign in to Beep</h4><p>Your browser opens to sign in or create an account.</p>
        <button class="dbtn pri" data-dact="dset:link:browser">Sign in</button>${ownLink}`,
      browser: `<h4>Sign in to Beep</h4><div class="wait"><span class="dspin"></span>Waiting for your browser…</div>
        <label class="dfield" style="width:100%;text-align:left">Token<input class="mono" autocomplete="off" spellcheck="false" placeholder="Paste it if the link did not open"></label>
        <div style="display:flex;gap:8px"><button class="dbtn" data-toast="Opens the browser again">Open again</button><button class="dbtn quiet" data-dact="dset:link:signin">Cancel</button></div>`,
      signfail: `<h4>Sign in to Beep</h4><p style="color:var(--d-err)">That sign-in expired or was for another device.</p>
        <button class="dbtn pri" data-dact="dset:link:browser">Try again</button>${ownLink}`,
      ownserver: `<h4>Use my own server</h4>
        <label class="dfield on" style="width:100%;text-align:left">Link code<span class="f mono"><span class="caret" style="height:16px;margin:0 1px 0 0"></span><span class="ph">From beep-server link</span></span><span>Run beep-server link on your server and paste its code.</span></label>
        <div style="display:flex;gap:8px"><button class="dbtn" data-dact="dset:link:signin">Back</button><button class="dbtn pri" data-toast="Connecting to your server">Connect<kbd>${DI.ret}</kbd></button></div>`,
      waiting: `${head}<div class="code">7K4 PQ2</div>
        <div class="wait"><span class="dspin"></span>Waiting for phone…</div><p style="font-size:12px">Code expires in <span id="dcount" style="font-variant-numeric:tabular-nums">${mmss(S.d.left)}</span></p>${own}`,
      join: `<h4>Join an account</h4>
        <label class="dfield on" style="width:100%;text-align:left">Join code<span class="f mono"><span class="caret" style="height:16px;margin:0 1px 0 0"></span><span class="ph">K7QM4-X9TRP</span></span><span>The code your other computer shows under Settings, Devices.</span></label>
        <p style="font-size:12px">This Mac's chats and memories move to that account.</p>
        <div style="display:flex;gap:8px"><button class="dbtn" data-dact="dset:link:waiting">Back</button><button class="dbtn pri" data-toast="Joining the other computer's account">Join<kbd>${DI.ret}</kbd></button></div>`,
      linked: `<h4>Linked</h4><p>This computer now sees your chats and memories.</p>${qr}`,
      expired: `${head}<button class="dbtn pri" data-dact="newCode">New code</button><p style="font-size:12px">A code lasts ten minutes.</p>${own}`,
    }[st];
    return deskScene(`<div class="dwin" role="dialog" aria-label="${signing ? "Sign in" : "Link to phone"}" style="top:32px;width:460px"><div class="tb"><span class="tl"><i></i><i></i><i></i></span><span class="name">${signing ? "Sign in" : "Link to phone"}</span></div><div class="in" style="gap:10px;padding-bottom:18px">${body}</div></div>`);
  },
  // Opened from Settings, Devices on any linked Mac. The phone that scans it joins this account.
  dInvite: () => deskScene(`<div class="dwin" role="dialog" aria-label="Link a phone"><div class="tb"><span class="tl"><i></i><i></i><i></i></span><span class="name">Link a phone</span></div>
    <div class="in"><h4>Link a phone</h4><p>Scan with Beep on your phone: Settings, Linked devices. The phone's chats and memories move to this Mac's account.</p>
      <div class="qr">${qrSvg()}</div><div class="code big">K7QM4-X9TRP</div>
      <p style="font-size:12px">A code lasts ten minutes.</p>
      <div style="display:flex;gap:8px"><button class="dbtn" data-dgo="dEmpty">Back</button><button class="dbtn pri" data-toast="Code copied">Copy code</button></div></div></div>`),
  dInput: () => deskScene(`<div class="dwin" role="dialog" aria-label="Input Monitoring" style="width:440px"><div class="tb"><span class="tl"><i></i><i></i><i></i></span><span class="name">Beep</span></div>
    <div class="in" style="justify-items:start;text-align:left;gap:14px">
      <div style="display:flex;gap:12px;align-items:center"><span class="dlogo" style="width:40px;height:40px;font-size:22px;border-radius: 8px;background:#0B0E1D;color:#fff">${DI.tray}</span><h4 style="font-size:19px">Option-Option needs Input Monitoring</h4></div>
      <p style="max-width:none">macOS has to let Beep see key presses in other apps before a double tap on Option can open the bar. Beep only watches the Option key.</p>
      <div style="width:100%;display:grid;gap:4px"><span style="font-size:11px;color:var(--d-dim)">System Settings, Privacy and Security, Input Monitoring</span>
        <div class="dcard" style="display:flex;align-items:center;gap:10px;padding:8px 12px"><span class="dlogo" style="width:22px;height:22px;font-size:12px;border-radius:6px;background:#0B0E1D;color:#fff">${DI.tray}</span><span style="flex:1;font-size:13px">Beep</span><span style="width:30px;height:18px;border-radius:9px;background:var(--d-border);position:relative"><span style="position:absolute;left:2px;top:2px;width:14px;height:14px;border-radius:50%;background:var(--d-fg)"></span></span></div></div>
      <div style="display:flex;gap:8px;flex-wrap:wrap"><button class="dbtn pri" data-toast="Opening System Settings">Open System Settings</button><button class="dbtn" data-dgo="dEmpty" data-toast="Option Space opens the bar">Use <kbd>⌥Space</kbd> for now</button></div>
      <p style="font-size:12px;max-width:none">Until then, <kbd>⌥Space</kbd> opens the bar. Turn Option-Option on later from the Beep menu.</p>
    </div></div>`),
  dNotif: () => deskScene(`<div class="nbanner" role="alert"><div class="main" data-dgo="dDone"><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Beep</b><small>now</small></div><b>Morning briefing</b><span>Three meetings today, the first at 10:30. Rain until 11:00, then dry. Sam moved lunch to 13:30.</span></div></div></div>`),
  // An update downloads quietly. This notice says once that it is ready; the tray menu then offers the restart, and quitting installs it too.
  dNotifUpdate: () => deskScene(`<div class="nbanner" role="alert"><div class="main" data-dgo="dTray"><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Beep</b><small>now</small></div><b>Beep 0.3.0 is ready</b><span>Choose Restart to Update Beep in the menu bar, or it installs the next time Beep quits.</span></div></div></div>`, { badge: true }),
  dNotifApprove: () => deskScene(`<div class="nbanner" role="alert"><div class="main" data-dgo="dApprove"><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Beep</b><small>now</small></div><b>Allow Fastmail: send email?</b><span>To Sam Rivera: Lunch on Saturday. Can we move lunch to 13:30?</span></div></div>
      <div class="side"><button data-dact="nAllow">Allow</button><button data-dact="nDeny">Deny</button></div></div>`, { badge: true }),
};
