// ---- Desktop (macOS tray client) ----
const DESK_SCREENS = [
  { group: "Floating bar", items: [] },
  { group: "Menu bar and windows", items: [
    ["dTray", "Tray menu", "TrayMenu"],
    ["dSettings", "Settings", "SettingsView"],
    ["dInput", "Input Monitoring", "HotkeyPrompt"],
  ]},
  { group: "Machines", items: [
    ["dMachines", "Machines", "MachinesView"],
  ]},
  { group: "Notifications", items: [
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
  pin: '<svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  chevD: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
};
const DOCK = ["#3E6C8F", "#C9A24B", "#7C5BB5", "#4F8A5B", "#B8574B", "#5A6B76", "#2F6F6A"];

function mbar(trayOn = false, menu = "", badge = false) {
  return `<div class="mbar"><b>Finder</b><span class="apps-menu"><span>File</span><span>Edit</span><span>View</span><span>Go</span><span>Window</span><span style="position:relative;cursor:default" data-dact="dset:helpm:${S.d.helpm ? '' : '1'}" role="button" aria-haspopup="menu" aria-expanded="${!!S.d.helpm}">Help${S.d.helpm ? `<div class="nmenu" role="menu" aria-label="Help" style="left:-8px;right:auto;top:20px;width:210px"><button class="it" role="menuitem" data-dact="dset:helpm:" data-dgo="dReport"><span></span><span>Report a Problem…</span><span class="k"></span></button></div>` : ""}</span></span>
    <span class="right"><span style="position:relative;display:grid">
      <button class="tray${trayOn ? " on" : ""}" data-dgo="${trayOn ? "dHome" : "dTray"}" aria-label="Beep menu" aria-expanded="${trayOn}">${DI.tray}</button>
      ${badge ? '<span style="position:absolute;top:2px;right:3px;width:6px;height:6px;border-radius:50%;background:var(--d-coral)"></span>' : ""}${menu}</span>
      ${DI.wifi}${DI.batt}<span class="clock">Wed 7 Oct 09:41</span></span></div>`;
}
function deskScene(content, o = {}) {
  return mbar(o.tray, o.menu, o.badge) + `<div class="ghostwin"></div><div class="ghostwin b"></div>`
    + `<div class="dock" aria-hidden="true">${DOCK.map(c => `<i style="background:linear-gradient(160deg, ${c}, color-mix(in srgb, ${c} 55%, #000))"></i>`).join("")}</div>` + content;
}
const DHINTS = [["⌘N", "new agent"], ["esc", "to hide"]];
// The first two say the most; the rest give way when the row is narrow. A session in the bar opens in the window from here.
const dHints = (items = DHINTS) => `<div class="dhints">${items.map(([k, t], i) => `<span${i >= 2 ? ' class="lo"' : ""}><kbd>${k}</kbd>${t}</span>`).join("")}${items.at(-1)?.[1] === "to hide" ? '<button class="win" data-dgo="dAgentSession"><kbd>⌘O</kbd>open in window</button>' : ""}</div>`;
const dBar = inner => `<div class="dbar" role="dialog" aria-label="Beep">${inner}</div>`;
// A step says what it is doing while it runs and what it did once done, as on the phone:
// "Reading file" becomes "Read file".
const D_PAST = { searching: "Searched", reading: "Read", checking: "Checked", looking: "Looked", fetching: "Fetched", opening: "Opened", saving: "Saved", finding: "Found",
  getting: "Got", creating: "Created", updating: "Updated", deleting: "Deleted", sending: "Sent", setting: "Set", using: "Used", remembering: "Remembered",
  loading: "Loaded", browsing: "Browsed", scheduling: "Scheduled", running: "Ran", weighing: "Weighed", recalling: "Recalled", forgetting: "Forgot", changing: "Changed",
  logging: "Logged", starting: "Started", messaging: "Messaged", waiting: "Waited", stopping: "Stopped", planning: "Planned" };
const dPast = label => { const t = label.replace(/[.\u2026\s]+$/, ""), i = t.indexOf(" "), head = i < 0 ? t : t.slice(0, i), past = D_PAST[head.toLowerCase()]; return past ? past + t.slice(head.length) : t; };

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
const dRows = (inner, hide) => `<div class="tools"${hide ? " hidden" : ""}>${inner}</div>`;



const D = {
  dTray: () => {
    const it = (label, key = "", attrs = "") => `<button class="it" role="menuitem" ${attrs}><span style="display:grid"></span><span>${label}</span><span class="k">${key}</span></button>`;
    const menu = `<div class="nmenu" role="menu" aria-label="Beep">
      ${it("Open Beep", "", `data-dgo="dHome"`)}
      ${it("Quick agent", "", `data-dgo="dWaiting"`)}
      <div class="hdr">Or press Option twice, anywhere</div>
      <hr>
      <div class="hdr">Waiting</div>
      ${it(`<span style="display:flex;gap:7px;align-items:center"><span class="ddot" style="background:var(--d-coral)"></span>Fix the flaky login test</span>`, "", `data-dact="dset:ags:asks" data-dgo="dBarSession"`)}
      <hr>
      ${it("Report a Problem", "", `data-dgo="dReport"`)}
      ${it("Settings", "⌘,", `data-dgo="dWinSettings"`)}
      ${it("Enable Option-Option", "", `data-dgo="dInput"`)}
      ${S.d.update ? it("Restart to Update Beep", "", `title="Install Beep 0.3.0" data-toast="Restarting into Beep 0.3.0"`) : it("Check for Updates", "", `data-toast="Beep is up to date"`)}
      ${it("Show Logs", "", `data-toast="Opens the log folder in Finder"`)}
      <hr>
      ${it("Quit Beep", "⌘Q", `data-toast="Quit installs a downloaded update; disabled in the mock"`)}
    </div>`;
    return deskScene("", { tray: true, menu, badge: true });
  },
  dInput: () => deskScene(`<div class="dwin" role="dialog" aria-label="Input Monitoring" style="width:440px"><div class="tb"><span class="tl"><i></i><i></i><i></i></span><span class="name">Beep</span></div>
    <div class="in" style="justify-items:start;text-align:left;gap:14px">
      <div style="display:flex;gap:12px;align-items:center"><span class="dlogo" style="width:40px;height:40px;font-size:22px;border-radius: 8px;background:#0B0E1D;color:#fff">${DI.tray}</span><h4 style="font-size:19px">Option-Option needs Input Monitoring</h4></div>
      <p style="max-width:none">macOS has to let Beep see key presses in other apps before a double tap on Option can open the bar. Beep only watches the Option key.</p>
      <div style="width:100%;display:grid;gap:4px"><span style="font-size:11px;color:var(--d-dim)">System Settings, Privacy and Security, Input Monitoring</span>
        <div class="dcard" style="display:flex;align-items:center;gap:10px;padding:8px 12px"><span class="dlogo" style="width:22px;height:22px;font-size:12px;border-radius:6px;background:#0B0E1D;color:#fff">${DI.tray}</span><span style="flex:1;font-size:13px">Beep</span><span style="width:30px;height:18px;border-radius:9px;background:var(--d-border);position:relative"><span style="position:absolute;left:2px;top:2px;width:14px;height:14px;border-radius:50%;background:var(--d-fg)"></span></span></div></div>
      <div style="display:flex;gap:8px;flex-wrap:wrap"><button class="dbtn pri" data-toast="Opening System Settings">Open System Settings</button><button class="dbtn" data-dgo="dWaiting" data-toast="Option Space opens the bar">Use <kbd>⌥Space</kbd> for now</button></div>
      <p style="font-size:12px;max-width:none">Until then, <kbd>⌥Space</kbd> opens the bar. Turn Option-Option on later from the Beep menu.</p>
    </div></div>`),
  // An update downloads quietly. This notice says once that it is ready; the tray menu then offers the restart, and quitting installs it too.
  dNotifUpdate: () => deskScene(`<div class="nbanner" role="alert"><div class="main" data-dgo="dTray"><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Beep</b><small>now</small></div><b>Beep 0.3.0 is ready</b><span>Choose Restart to Update Beep in the menu bar, or it installs the next time Beep quits.</span></div></div></div>`, { badge: true }),
};

// Machines (ListMachines): each one's state, system and the coding tools on it. This Mac's switch stays in Settings.
const dMacAgTile = (a, cls) => `<span class="${cls} ${a}${window.__MARKS?.[a] ? " mark" : ""}">${window.__MARKS?.[a] || a.slice(0, 2).toUpperCase()}</span>`;
D.dMachines = () => {
  const add = `<button class="dbtn" data-dgo="dAddMachine">Add a machine</button>`;
  const head = say => `<div class="dhead"><span class="tt"><b>Machines</b><small>${say}</small></span>${add}</div>`;
  if (S.d.machines === "none") return deskScene(dBar(head("Where Beep runs commands and coding agents") + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${I.server}</span><b style="font-size:14px">No machines yet.</b><span class="dmuted" style="display:block">Add one to run commands and coding agents on it.</span></div>` + dHints([["esc", "back"]])));
  const COLS = ["claude", "codex", "opencode"];
  const list = [
    { name: "build-box", online: true, spec: "Ubuntu 24.04 · x86_64 · 16 CPU · 64 GB", ver: "0.4.1", behind: true, agents: [["claude", "2.1.4", true], ["codex", "0.48.0", false], ["opencode", "1.19.4", null]], tools: ["gh", "docker", "git", "cargo", "just"] },
    { name: "gpu-box", online: false, seen: "Seen 3 h ago", spec: "Debian 13 · x86_64 · 32 CPU · 128 GB", ver: "0.4.2", agents: [], tools: ["python3", "nvidia-smi", "git"] },
    { name: "ci-runner", online: true, temp: true, spec: "Debian 13 · x86_64 · 4 CPU · 8 GB", ver: "0.4.2", agents: [["claude", "2.1.4", true]], tools: ["git", "cargo"], adapters: [["Codex", "could not download Node.js: no curl or wget"], ["Gemini", null]] },
  ];
  const card = m => {
    const asking = S.d.mrmb === m.name;
    const remove = asking ? "" : `<button class="dbtn quiet" data-dact="dset:mrmb:${m.name}" data-dgo="dMachines" aria-label="Remove ${m.name}">Remove…</button>`;
    const what = m.online ? "It stops taking work; its sessions and projects go with it." : "Its sessions and projects go with it. If it comes back online, it is turned away.";
    const ask = asking ? `<div class="ask"><span>Remove ${m.name} from your machines? ${what} To use it again, add it with a new setup command.</span>
      <span class="acts"><button class="dbtn warn" data-dact="dset:mrmb:" data-dgo="dMachines" data-toast="${m.name} removed">Remove</button><button class="dbtn quiet" data-dact="dset:mrmb:" data-dgo="dMachines">Cancel</button></span></div>` : "";
    const l1 = `<span class="l1"><span class="ddot${m.online ? "" : " off"}" title="${m.online ? "Online" : "Offline"}"></span><b>${m.name}</b>${m.temp ? '<span class="dtag" title="Removed after a day offline">container</span>' : ""}${m.seen ? `<span class="seen">${m.seen}</span>` : ""}<span class="spec" title="${m.spec}">${m.spec}</span><span class="dmnv" title="${m.behind ? `beep-node ${m.ver}; 0.4.2 is out` : `beep-node ${m.ver}`}">beep-node ${m.ver}${m.behind ? '<span class="up">update</span>' : ""}</span>${remove}</span>`;
    const l2 = m.agents.length ? m.agents.map(([a, v, s]) => `<span class="dmag" style="grid-column:${COLS.indexOf(a) + 1}" title="${a} ${v}${s === null ? "" : s ? ", signed in" : ", not signed in"}">${dMacAgTile(a, "dmag-ic")}<span class="v">${v}</span>${s === null ? "" : `<span class="${s ? "in" : "out"}"></span>`}</span>`).join("") : '<span class="none">No coding agents</span>';
    // An agent whose ACP adapter the node is installing, or could not install (#425).
    const l4 = (m.adapters ?? []).map(([n, why]) => `<span class="dmad${why ? " bad" : ""}" title="${why ?? ""}">${why ? `Could not install the ${n} adapter: ${why}` : `Installing the ${n} adapter`}${why ? `<button class="dbtn small" data-toast="Trying ${n} again on ${m.name}">Try again</button>` : ""}</span>`).join("");
    return `<div class="dmach">${l1}${l2}<span class="l3">${m.tools.map(t => `<span class="dmt-t">${t}</span>`).join("")}</span>${l4}${ask}</div>`;
  };
  return deskScene(dBar(head("2 online, 1 offline") + `<div class="dlist"><div class="dmt" style="--ag:${COLS.length}">${list.map(card).join("")}</div></div>` + dHints([["esc", "back"]])));
};
VARIANTS.dMachines = [["Two machines", () => { S.d.machines = "list"; render(); }, () => S.d.machines !== "none"], ["None", () => { S.d.machines = "none"; render(); }, () => S.d.machines === "none"]];