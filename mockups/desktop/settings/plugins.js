// ---- Settings, Plugins, as settings/plugins-settings-view.tsx draws it ----
// A list of the plugins in the plugins folder with an on/off switch each, the actions that
// bring one in (Install from Git, Install from folder) and the consent sheet that every
// install, update and Allow goes through. A phone runs no plugins and shows no entry.
// Sub-states in S.d.plg: "" (the list), "none", "loading", "failed", "form", "form-err",
// "fetching", "consent", "update", "allow", "consent-err", "remove", "remove-err".
const SETTINGS_PLUGINS = (() => {
  document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="settings/plugins.css">');
  const { swi, skel } = SETTINGS_ACCOUNTS;
  const go = k => `data-dact="dset:plg:${k}" data-dgo="dWinSettings"`;
  const PUZZLE = `<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10 4a2 2 0 1 1 4 0v2h4a1 1 0 0 1 1 1v4h-2a2 2 0 1 0 0 4h2v4a1 1 0 0 1-1 1h-4v-2a2 2 0 1 0-4 0v2H6a1 1 0 0 1-1-1v-4h2a2 2 0 1 0 0-4H5V7a1 1 0 0 1 1-1h4z"/></svg>`;
  const LABEL = { "ui.add": "Add to the interface", "sessions.read": "Read your sessions" };
  const MEANS = { "ui.add": "Show text, buttons and images in the message box, session rows and session header.", "sessions.read": "See your coding agent sessions and what each one is doing." };
  const chip = t => `<span class="dgchip">${t}</span>`;
  const btn = (label, attrs, cls = "") => `<button class="dbtn small${cls ? ` ${cls}` : ""}" ${attrs}>${label}</button>`;
  const rows = [
    { name: "Composer cat", version: "0.2.0", desc: "A cat sits on the message box.", perms: ["ui.add"], src: "github.com/owner/composer-cat at 4f0915a", on: true, state: ["ok", "On"], git: true },
    { name: "Session notes", version: "0.1.0", desc: "A note on each session's row, set from the session header.", perms: ["ui.add", "sessions.read"], src: "Local folder", on: true, state: ["", "Waiting for its slot"] },
    { name: "Idle timer", version: "1.0.3", desc: "Shows how long a session has been quiet.", perms: ["ui.add"], src: "github.com/owner/beep-plugins at 91c2e07, plugins/idle-timer", on: false, state: ["", "Off"], git: true },
    { name: "Quote board", version: "0.4.0", desc: "Adds a button that asks for sessions it was not allowed to read.", perms: ["ui.add", "sessions.read"], src: "github.com/owner/quote-board at 0b77d1c", on: false, state: ["warn", "Needs approval"], git: true, allow: true },
    { name: "Dev sketch", version: "0.0.1", desc: "Loaded from the plugins folder you set for development.", perms: ["ui.add"], src: "Dev folder", on: true, state: ["err", "Stopped: used 612 ms of script time in a minute"] },
  ];
  const row = (r, extra = "") => `<div class="dgit plg${r.on ? "" : " off"}"><span class="ic">${PUZZLE}</span>
    <div class="l1"><b>${r.name}</b><span class="acct">${r.version}</span>${swi(r.on && !r.allow, `${r.name} on or off`, `data-dact="dset:plg:${r.on ? "off" : "on"}"${r.allow ? " disabled" : ""}`)}</div>
    <span class="plg-desc">${r.desc}</span>
    <div class="l2">${r.perms.map(p => chip(LABEL[p])).join("")}</div>
    <div class="dline plg-foot"><span class="plg-state ${r.state[0]}">${r.state[1]}</span><span class="dsub plg-src">${r.src}</span>
      <span class="acts">${r.allow ? btn("Allow…", go("allow"), "pri") : ""}${r.git ? btn("Update", go("update")) : ""}${btn("Remove…", go("remove"), "quiet")}</span></div>${extra}</div>`;
  const top = `<div class="dline tight wrap plg-top">${btn("Install from Git…", go("form"))}${btn("Install from folder…", go("consent"))}<span class="dpush">${btn("Open plugins folder", 'data-toast="Opens the plugins folder"', "quiet")}</span></div>`;
  const intro = `<p class="plg-intro">Plugins add to the interface of this computer. A plugin runs only after you allow what it asks for, and you can turn it off or remove it here.</p>`;
  const form = err => `<div class="plg-form"><div class="dline"><b>Install from Git</b></div>
    <label class="dfield${err ? " err" : ""}"><span>Repository</span><input class="url" placeholder="owner/repo or https://github.com/owner/repo.git" value="${err ? "owner/missing-repo" : ""}" autocomplete="off" spellcheck="false"></label>
    <div class="plg-two"><label class="dfield"><span>Branch or tag (optional)</span><input placeholder="main" autocomplete="off" spellcheck="false"></label>
    <label class="dfield"><span>Folder in the repository (optional)</span><input placeholder="plugins/cat" autocomplete="off" spellcheck="false"></label></div>
    ${err ? `<div class="derr">Git could not read that repository. It may not exist, or it may be private and need a sign-in.</div>` : ""}
    <div class="dline tight">${btn("Review plugin", go("consent"), "pri")}${btn("Cancel", go(""), "quiet")}</div></div>`;
  const fetching = `<div class="plg-form"><div class="dline"><b>Install from Git</b></div><div class="dmuted"><span class="dspin"></span>Fetching owner/composer-cat. Nothing is installed yet.</div></div>`;
  const empty = `<div class="plg-empty"><b>No plugins yet</b><p>Plugins are small scripts that add to Beep's interface, such as a cat on the message box. Install one from a Git repository or from a folder on this computer.</p>
    <div class="dline tight">${btn("Install from Git…", go("form"), "pri")}${btn("Install from folder…", go("consent"))}${btn("Open plugins folder", 'data-toast="Opens the plugins folder"', "quiet")}</div></div>`;
  const body = v => {
    if (v === "loading") return skel(3);
    if (v === "failed") return `<div class="derr">Could not load your plugins. Plugins are not running.</div><div class="dline">${btn("Try again", go(""))}</div>`;
    if (v === "none") return `${intro}${empty}`;
    const failed = v === "row-err" ? `<span class="derr">Could not turn it off: cannot save the plugin's record.</span>` : "";
    const list = rows.map((r, i) => row(r, i === 0 ? failed : "")).join("");
    return `${intro}${top}${v === "form" || v === "form-err" ? form(v === "form-err") : v === "fetching" ? fetching : ""}<div class="plg-list">${list}</div>`;
  };
  // The consent sheet: install and update show where it came from; Allow shows what is not allowed yet.
  const consent = v => {
    const upd = v === "update", allow = v === "allow";
    const title = upd ? "Update Composer cat?" : allow ? "Allow Quote board?" : "Install Composer cat?";
    const perms = allow ? ["ui.add", "sessions.read"] : upd ? ["ui.add", "sessions.read"] : ["ui.add"];
    const fresh = upd ? ["sessions.read"] : allow ? ["ui.add", "sessions.read"] : [];
    const action = upd ? "Allow and update" : allow ? "Allow" : "Allow and install";
    const meta = upd ? `<b>Composer cat</b> 0.1.0 to 0.2.0<br>From github.com/owner/composer-cat, commit 4f0915a` : allow ? `<b>Quote board</b> 0.4.0<br>From github.com/owner/quote-board, commit 0b77d1c` : `<b>Composer cat</b> 0.2.0<br>From github.com/owner/composer-cat, commit 4f0915a`;
    const li = p => `<li><b>${LABEL[p]}${fresh.includes(p) ? ' <span class="dgchip warn">New</span>' : ""}</b><small>${MEANS[p]}</small></li>`;
    const err = v === "consent-err" ? `<div class="derr">A plugin with the id composer-cat is already installed from another source. Remove it first.</div>` : "";
    return `<div class="plg-sheet"><div class="am-head"><b>${title}</b><kbd>esc</kbd></div><div class="plg-meta">${meta}</div>
      <div class="am-label" style="margin:0">It asks to:</div><ul class="plg-perms">${perms.map(li).join("")}</ul>
      <div class="am-sub" style="margin:0">A plugin is a script that runs on this computer in a sandbox. It can only do what is listed here.</div>${err}
      <div class="gp-foot" style="margin-top:4px">${btn("Cancel", go(""), "quiet")}${btn(action, go(""), "pri")}</div></div>`;
  };
  const removal = v => `<div class="plg-sheet"><div class="am-head"><b>Remove Idle timer?</b><kbd>esc</kbd></div>
    <div class="am-msg" style="margin:0">Beep stops the plugin and deletes its folder from the plugins folder.</div>
    <div class="am-sub" style="margin:0">You can install it again later.</div>${v === "remove-err" ? `<div class="derr">cannot delete the plugin's folder: permission denied</div>` : ""}
    <div class="gp-foot" style="margin-top:4px">${btn("Cancel", go(""), "quiet")}${btn("Remove", go(""), "warn")}</div></div>`;
  const sheet = v => ["consent", "update", "allow", "consent-err"].includes(v) ? consent(v) : ["remove", "remove-err"].includes(v) ? removal(v) : "";
  // The sheets open over Settings, as the app's sheets do.
  const base = D.dWinSettings;
  D.dWinSettings = () => {
    const html = base();
    const over = S.d.set === "plugins" ? sheet(S.d.plg || "") : "";
    if (!over) return html;
    const scrim = `<div class="am-scrim" style="position:absolute;z-index:40;padding-top:90px"><div class="am-sheet" role="dialog" aria-modal="true" aria-label="Plugins">${over}</div></div>`;
    return html.replace(/<\/div><\/div>\s*$/, `${scrim}</div></div>`).replace('class="cc-win"', 'class="cc-win" style="position:relative"');
  };
  return { body };
})();
