import { $, bar, esc, h, ICON, invoke, listen, S } from "./core.ts";
import { hints } from "./hints.ts";

// ---------- Apps ----------

/// The Apps view: every app, the one being connected and the one whose
/// permissions are open.
export const A = { apps: null, error: "", connecting: null, poll: null, open: null, perms: null,
  // MCP servers added by URL: null until listed, the add form, and the one
  // whose sign-in is open in the browser.
  servers: null, adding: false, addError: "", waiting: null, busy: null,
  // The server whose tools are open, its tools once listed, the tool
  // waiting on the warning about going past the limit, and whether
  // Remove waits for a second click.
  server: null, tools: null, asking: null, unadd: false,
  // Fastmail's accounts by app slug, the agent's default first; missing
  // when the server is too old to list them. `removing` is the account
  // whose Remove waits for a second click.
  accounts: {}, removing: null };

export const APP_LEVELS = [
  ["ask", "Ask first", "Beep shows what it is about to do and waits for you."],
  ["allow", "Always allow", "Beep sends, changes and deletes without asking."],
  ["read", "Read only", "Beep can search and read, never change anything."],
];

export function appLogo(app) {
  const letter = esc(app.name.slice(0, 1).toUpperCase());
  return app.logo_url ? `<img class="dalogo" src="${esc(app.logo_url)}" alt="" data-letter="${letter}">` : `<span class="dalogo">${letter}</span>`;
}
/// A logo that does not load becomes the app's first letter.
export function logoFallbacks(root) {
  root.querySelectorAll("img.dalogo").forEach(img => img.addEventListener("error", () => img.replaceWith(h(`<span class="dalogo">${img.dataset.letter}</span>`)), { once: true }));
}

export function renderApps() {
  S.view = "apps";
  A.open = null;
  bar.innerHTML = `<div class="dhead"><span class="tt"><b>Apps</b><small>Connect once, then just ask from any chat</small></span></div>
    <div class="dlist dapps" id="alist"></div>${hints([["esc", "back"]])}`;
  $("#alist").addEventListener("click", appsClick);
  drawApps();
  loadApps();
}

export async function loadApps() {
  try {
    A.apps = await invoke("list_apps");
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  // A server too old to know them leaves the section out.
  const [servers] = await Promise.all([
    invoke("list_mcp_servers").catch(() => A.servers),
    ...(A.apps ?? []).filter(a => a.kind === "fastmail").map(loadAccounts),
  ]);
  A.servers = servers;
  if (A.waiting && A.servers?.find(m => m.id === A.waiting)?.connected) A.waiting = null;
  // A Composio sign-in ends on the web page; the app shows up connected here.
  if (A.connecting && A.apps?.find(a => a.slug === A.connecting)?.connected) stopConnecting();
  if (S.view === "apps") drawApps();
}

export async function loadAccounts(app) {
  const accounts = app.connected ? await invoke("list_app_accounts", { slug: app.slug }).catch(() => null) : [];
  if (accounts) A.accounts[app.slug] = accounts;
  else delete A.accounts[app.slug];
}

/// The addresses of a connected app's accounts, for its row in Apps.
export function accountList(app) {
  const accounts = A.accounts[app.slug];
  return accounts?.length ? accounts.map(a => a.address ?? `${app.name} account`).join(", ") : null;
}

export function drawApps() {
  const list = $("#alist");
  if (!list) return;
  const apps = A.apps ?? [];
  const row = app => {
    let end;
    if (A.connecting === app.slug) end = waitingFor("data-cancel");
    else if (app.connected) end = `<span class="dtag">Connected</span><span class="dmuted">${ICON.chev}</span>`;
    else if (!app.configured) end = '<span class="dmuted">Unavailable</span>';
    else end = `<button class="dbtn pri" data-connect="${esc(app.slug)}"${A.connecting ? " disabled" : ""}>Connect</button>`;
    return `<div class="dappr${app.connected ? " on" : ""}" ${app.connected ? `data-open="${esc(app.slug)}" role="button" tabindex="0"` : ""}>
      ${appLogo(app)}<span class="tt"><b>${esc(app.name)}</b><small>${esc((app.connected && accountList(app)) || app.description)}</small></span>${end}</div>`;
  };
  const connected = apps.filter(a => a.connected), more = apps.filter(a => !a.connected);
  let html = A.error ? `<div class="derr" style="padding:8px 10px">${esc(A.error)}</div>` : "";
  if (!A.apps && !A.error) html += '<div class="dmuted" style="padding:14px 10px"><span class="dspin"></span></div>';
  if (connected.length) html += '<div class="dsec">Connected</div>' + connected.map(row).join("");
  if (more.length) html += `<div class="dsec">${connected.length ? "More apps" : "Available"}</div>` + more.map(row).join("");
  if (A.servers) html += drawServers();
  // Redrawing keeps what is typed in the add form.
  const typed = { name: $("#mname")?.value, url: $("#murl")?.value }, focus = document.activeElement?.id;
  list.innerHTML = html;
  logoFallbacks(list);
  if (typed.name != null && $("#mname")) $("#mname").value = typed.name;
  if (typed.url != null && $("#murl")) $("#murl").value = typed.url;
  if (focus && $("#" + focus)) $("#" + focus).focus();
}

export function hostOf(url) {
  try { return new URL(url).host; } catch { return url; }
}

/// A sign-in open in the browser, with the Cancel that `attr` marks.
export const waitingFor = attr => `<span class="dmuted"><span class="dspin"></span> Waiting for browser…</span><button class="dbtn quiet" ${attr}>Cancel</button>`;

/// The servers the user added, then the add form or its button.
export function drawServers() {
  const row = m => {
    let end;
    if (A.waiting === m.id) end = waitingFor("data-mcancel");
    else if (A.busy === m.id) end = '<span class="dmuted"><span class="dspin"></span></span>';
    else if (m.connected) end = `<span class="dtag ok">${m.tools.length === 1 ? "1 tool" : `${m.tools.length} tools`}</span><span class="dmuted">${ICON.chev}</span>`;
    else end = `<span class="dtag ask">${m.sign_in ? "Sign-in expired" : "Not connected"}</span><button class="dbtn pri" data-mconnect="${esc(m.id)}">${m.sign_in ? "Sign in" : "Retry"}</button>`
      + `<button class="dbtn quiet" data-mremove="${esc(m.id)}">Remove</button>`;
    const open = m.connected && A.waiting !== m.id && A.busy !== m.id ? ` data-mopen="${esc(m.id)}" role="button" tabindex="0"` : "";
    return `<div class="dappr${open ? " on" : ""}"${open}><span class="dalogo">${esc(m.name.slice(0, 1).toUpperCase())}</span><span class="tt"><b>${esc(m.name)}</b><small>${esc(hostOf(m.url))}</small></span>${end}</div>`;
  };
  let html = '<div class="dsec">Your MCP servers</div>' + A.servers.map(row).join("");
  if (A.adding) {
    const busy = A.busy === "new";
    html += `<div class="dform dmcp">
      <label class="dfield"><span>Name</span><input id="mname" maxlength="60" placeholder="Team notes" autocomplete="off" spellcheck="false"${busy ? " disabled" : ""}></label>
      <label class="dfield${A.addError ? " err" : ""}"><span>${A.addError ? esc(A.addError) : "Server URL"}</span><input id="murl" class="url" maxlength="2000" placeholder="https://example.com/mcp" autocomplete="off" spellcheck="false"${busy ? " disabled" : ""}></label>
      <div style="display:flex;gap:8px;align-items:center">${busy ? '<span class="dmuted"><span class="dspin"></span> Connecting…</span>'
        : `<button class="dbtn pri" data-madd>Add server<kbd>${ICON.ret}</kbd></button><button class="dbtn quiet" data-mclose>Cancel</button>`}</div></div>`;
  } else {
    html += `<div style="padding:4px 10px"><button class="dbtn" data-mnew>Add MCP server</button></div>`;
  }
  return html + '<div class="dmuted" style="padding:6px 10px;display:block">Beep asks before every tool on a server you add.</div>';
}

export async function addServer() {
  const name = $("#mname")?.value.trim(), url = $("#murl")?.value.trim();
  if (!name || !url) return (name ? $("#murl") : $("#mname"))?.focus();
  A.busy = "new";
  A.addError = "";
  drawApps();
  try {
    const server = await invoke("add_mcp_server", { name, url });
    A.adding = false;
    if (!server.connected) A.waiting = server.id;
  } catch (e) {
    A.addError = String(e);
  }
  A.busy = null;
  await loadApps();
  if (A.adding) $("#murl")?.focus();
}

export async function serverAction(kind, id) {
  A.busy = id;
  A.error = "";
  drawApps();
  try {
    if (kind === "connect") {
      const server = await invoke("connect_mcp_server", { id });
      if (!server.connected) A.waiting = id;
    } else {
      await invoke("remove_mcp_server", { id });
    }
  } catch (e) {
    A.error = String(e);
  }
  A.busy = null;
  loadApps();
}

// The link back from a server's sign-in, which Rust already handed on.
listen("mcp-signed-in", e => {
  A.waiting = null;
  if (e.payload) A.error = `Sign-in did not finish: ${e.payload}`;
  if (S.view === "apps") loadApps();
});

export async function appsClick(e) {
  const m = e.target.closest("[data-mnew],[data-mclose],[data-madd],[data-mcancel],[data-mconnect],[data-mremove],[data-mopen]");
  if (m) {
    const d = m.dataset;
    if (d.mnew != null) { A.adding = true; A.addError = ""; drawApps(); return $("#mname")?.focus(); }
    if (d.mclose != null) { A.adding = false; A.addError = ""; return drawApps(); }
    if (d.madd != null) return addServer();
    if (d.mcancel != null) { A.waiting = null; return drawApps(); }
    if (d.mconnect) return serverAction("connect", d.mconnect);
    if (d.mremove) return serverAction("remove", d.mremove);
    if (d.mopen) return renderMcpServer(A.servers.find(s => s.id === d.mopen));
  }
  if (e.target.closest("[data-cancel]")) {
    invoke("cancel_app_sign_in").catch(() => {});
    return stopConnecting();
  }
  const connect = e.target.closest("[data-connect]");
  if (connect) return connectApp(A.apps.find(a => a.slug === connect.dataset.connect));
  const open = e.target.closest("[data-open]");
  if (open) return renderAppPermissions(A.apps.find(a => a.slug === open.dataset.open));
}

export async function connectApp(app) {
  if (!app || A.connecting) return;
  A.connecting = app.slug;
  A.error = "";
  drawApps();
  try {
    await invoke("connect_app", { slug: app.slug, kind: app.kind, add: false });
  } catch (e) {
    if (A.connecting === app.slug && String(e) !== "cancelled") A.error = String(e);
    stopConnecting();
    return;
  }
  // Fastmail is connected once the call returns. A Composio app finishes on
  // its web page, so look again until it shows up, for ten minutes at most.
  if (app.kind === "fastmail") return stopConnecting();
  const until = Date.now() + 10 * 60e3;
  A.poll = setInterval(() => {
    if (Date.now() > until || S.view !== "apps") return stopConnecting();
    loadApps();
  }, 3000);
}

export function stopConnecting() {
  clearInterval(A.poll);
  A.poll = null;
  A.connecting = null;
  loadApps();
}

// ---------- An MCP server's tools ----------

/// Opened from a connected server in Apps: its tools, each switched on or
/// off, and Remove.
export async function renderMcpServer(server) {
  if (!server) return;
  S.view = "mcp";
  Object.assign(A, { server, tools: null, asking: null, unadd: false, error: "" });
  bar.innerHTML = `<div class="dhead"><span class="dalogo">${esc(server.name.slice(0, 1).toUpperCase())}</span><span class="tt"><b>${esc(server.name)}</b><small class="dmono">${esc(server.url)}</small></span></div>
    <div class="dlist dapps" id="tlist"></div>${hints([["esc", "back"]])}`;
  $("#tlist").addEventListener("click", mcpClick);
  drawMcpTools();
  try {
    A.tools = await invoke("list_mcp_tools", { id: server.id });
  } catch (e) {
    A.error = String(e);
  }
  if (S.view === "mcp" && A.server === server) drawMcpTools();
}

/// "search_pages" reads as "Search pages", as on the phone.
export const mcpToolLabel = name => name.replace(/[_-]/g, " ").trim().replace(/^./, c => c.toUpperCase());

export function drawMcpTools() {
  const list = $("#tlist"), t = A.tools;
  if (!list) return;
  let html = A.error ? `<div class="derr" style="padding:8px 10px">${esc(A.error)}</div>` : "";
  if (!t && !A.error) html += '<div class="dmuted" style="padding:14px 10px"><span class="dspin"></span></div>';
  if (t) {
    const on = t.tools.filter(x => x.enabled).length;
    html += `<div class="dsec dsplit"><span>Tools</span><span${on > t.limit ? ' class="over"' : ""}>${on} of ${t.limit} on</span></div>` + t.tools.map(tool => {
      const name = esc(mcpToolLabel(tool.name)), title = tool.description ? ` title="${esc(tool.description)}"` : "";
      if (A.asking === tool.name) return `<div class="daction"><span>${name}<small>More than ${t.limit} tools makes replies slower and tool picks worse</small></span>`
        + `<button class="dbtn quiet" data-tno>Cancel</button><button class="dbtn pri" data-tyes>Turn on</button></div>`;
      if (!tool.usable) return `<div class="daction off"${title}><span>${name}<small>Too large for Beep to use</small></span><button class="dswitch" role="switch" aria-checked="false" aria-label="Use ${name}" disabled></button></div>`;
      return `<div class="daction"${title}><span>${name}</span><button class="dswitch" role="switch" aria-checked="${tool.enabled}" data-ttool="${esc(tool.name)}" aria-label="Use ${name}"></button></div>`;
    }).join("");
    html += `<div class="dmuted" style="padding:8px 10px;display:block">Beep uses up to ${t.limit} tools from a server, so turn on the ones you need. It asks before every tool on a server you add.</div>`;
  }
  html += `<div style="padding:6px 10px 2px;display:flex;gap:8px;align-items:center">${A.unadd
    ? '<span class="dmuted">Beep stops using this server</span><button class="dbtn quiet" data-unremove>Cancel</button><button class="dbtn warn" data-mremove>Remove</button>'
    : '<button class="dbtn warn" data-mremoving>Remove server</button>'}</div>`;
  list.innerHTML = html;
}

export async function setTool(name, enabled) {
  const server = A.server;
  A.asking = null;
  try {
    A.tools = await invoke("set_mcp_tool", { id: server.id, tool: name, enabled });
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  if (S.view === "mcp" && A.server === server) drawMcpTools();
}

export async function mcpClick(e) {
  const d = e.target.closest("[data-ttool],[data-tyes],[data-tno],[data-mremoving],[data-unremove],[data-mremove]")?.dataset;
  if (!d || !A.tools && d.ttool) return;
  if (d.ttool) {
    const tool = A.tools.tools.find(x => x.name === d.ttool);
    // Past the limit, the switch asks first, as the phone's does.
    if (!tool.enabled && A.tools.tools.filter(x => x.enabled).length >= A.tools.limit) { A.asking = tool.name; return drawMcpTools(); }
    return setTool(tool.name, !tool.enabled);
  }
  if (d.tyes != null) return setTool(A.asking, true);
  if (d.tno != null) { A.asking = null; return drawMcpTools(); }
  if (d.mremoving != null || d.unremove != null) { A.unadd = d.mremoving != null; return drawMcpTools(); }
  if (d.mremove != null) {
    try {
      await invoke("remove_mcp_server", { id: A.server.id });
    } catch (e) {
      A.error = String(e);
      return drawMcpTools();
    }
    return renderApps();
  }
}

export async function renderAppPermissions(app) {
  if (!app) return;
  S.view = "appperms";
  A.open = app;
  A.perms = null;
  bar.innerHTML = `<div class="dhead">${appLogo(app)}<span class="tt"><b>${esc(app.name)}</b><small>What Beep may do here</small></span></div>
    <div class="dlist dapps" id="plist"></div>${hints([["esc", "back"]])}`;
  logoFallbacks(bar);
  $("#plist").addEventListener("click", permsClick);
  A.removing = null;
  drawPerms();
  const accounts = app.kind === "fastmail" ? loadAccounts(app) : null;
  try {
    A.perms = await invoke("app_permissions", { slug: app.slug });
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  await accounts;
  if (S.view === "appperms" && A.open === app) drawPerms();
}

/// Each connected account with Remove, and a button that signs in another.
export function drawAccounts(app) {
  const accounts = A.accounts[app.slug];
  let html = '<div class="dsec">Accounts</div>' + accounts.map((a, i) => {
    const name = esc(a.address ?? `${app.name} account`);
    const first = i === 0 && accounts.length > 1 ? ' <small class="dmuted" style="display:inline">Default</small>' : "";
    const end = A.busy === "account:" + a.id ? '<span class="dmuted"><span class="dspin"></span></span>'
      : A.removing === a.id ? `<span class="dmuted">${accounts.length === 1 ? `Disconnects ${esc(app.name)}` : "Beep stops using this account"}</span>`
        + `<button class="dbtn quiet" data-unremove>Cancel</button><button class="dbtn warn" data-remove="${esc(a.id)}">Remove</button>`
      : `<button class="dbtn quiet" data-removing="${esc(a.id)}">Remove</button>`;
    return `<div class="daction"><span>${name}${first}</span>${end}</div>`;
  }).join("");
  html += A.connecting === app.slug
    ? `<div style="padding:4px 10px;display:flex;gap:8px;align-items:center">${waitingFor("data-cancel")}</div>`
    : '<div style="padding:4px 10px"><button class="dbtn" data-addaccount>Add another account</button></div>';
  return html + `<div class="dmuted" style="padding:6px 10px;display:block">Beep uses the first account unless you name another, like "check my work inbox". To add a different account, sign out of ${esc(app.name)} in the browser first.</div>`;
}

export async function addAccount(app) {
  if (A.connecting) return;
  A.connecting = app.slug;
  A.error = "";
  drawPerms();
  try {
    await invoke("connect_app", { slug: app.slug, kind: app.kind, add: true });
  } catch (e) {
    if (A.connecting === app.slug && String(e) !== "cancelled") A.error = String(e);
  }
  A.connecting = null;
  await loadAccounts(app);
  if (S.view === "appperms" && A.open === app) drawPerms();
}

export async function removeAccount(app, id) {
  A.busy = "account:" + id;
  A.removing = null;
  drawPerms();
  try {
    await invoke("remove_app_account", { slug: app.slug, id });
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }
  A.busy = null;
  await loadAccounts(app);
  if (S.view !== "appperms" || A.open !== app) return;
  // The last account gone disconnects the app.
  if (A.accounts[app.slug]?.length === 0) { A.apps = null; return renderApps(); }
  drawPerms();
}

export function drawPerms() {
  const list = $("#plist"), app = A.open, p = A.perms;
  if (!list || !app) return;
  let html = A.error ? `<div class="derr" style="padding:8px 10px">${esc(A.error)}</div>` : "";
  if (!p && !A.error) html += '<div class="dmuted" style="padding:14px 10px"><span class="dspin"></span></div>';
  if (p && A.accounts[app.slug]?.length) html += drawAccounts(app);
  if (p) {
    html += `<div class="dsec">When Beep acts in ${esc(app.name)}</div>` + APP_LEVELS.map(([level, title, detail]) =>
      `<button class="dlevel" role="radio" aria-checked="${p.level === level}" data-level="${level}"><span class="dradio"></span>
        <span class="tt"><b>${title}</b><small>${detail}</small></span></button>`).join("");
    if (p.actions.length) {
      html += '<div class="dsec">Actions</div>' + p.actions.map(a => {
        const [tag, cls] = p.level === "read" ? ["Off", ""] : p.level === "allow" || a.allowed ? ["Allowed", " ok"] : ["Asks", " ask"];
        const sw = p.level === "ask" ? `<button class="dswitch" role="switch" aria-checked="${a.allowed}" data-tool="${esc(a.tool)}" aria-label="Always allow ${esc(a.label)}"></button>` : "";
        return `<div class="daction"><span>${esc(a.label)}</span><span class="dtag${cls}">${tag}</span>${sw}</div>`;
      }).join("");
    }
    html += '<div class="dmuted" style="padding:8px 10px;display:block">Searching and reading is always allowed.</div>';
  }
  const all = (A.accounts[app.slug]?.length ?? 0) > 1 ? " (all accounts)" : "";
  html += `<div style="padding:6px 10px 2px"><button class="dbtn warn" id="disconnect">Disconnect ${esc(app.name)}${all}</button></div>`;
  list.innerHTML = html;
}

export async function permsClick(e) {
  const app = A.open, p = A.perms;
  if (e.target.closest("[data-cancel]")) {
    invoke("cancel_app_sign_in").catch(() => {});
    A.connecting = null;
    return drawPerms();
  }
  if (e.target.closest("[data-addaccount]")) return addAccount(app);
  const account = e.target.closest("[data-removing],[data-unremove],[data-remove]")?.dataset;
  if (account) {
    if (account.remove) return removeAccount(app, account.remove);
    A.removing = account.removing ?? null;
    return drawPerms();
  }
  if (e.target.closest("#disconnect")) {
    const b = e.target.closest("#disconnect");
    b.disabled = true;
    try {
      await invoke("disconnect_app", { slug: app.slug });
    } catch (err) {
      A.error = String(err);
      return drawPerms();
    }
    A.apps = null;
    return renderApps();
  }
  if (!p) return;
  const level = e.target.closest("[data-level]")?.dataset.level;
  const tool = e.target.closest("[data-tool]")?.dataset.tool;
  if (!level && !tool) return;
  const next = {
    level: level ?? p.level,
    actions: p.actions.map(a => a.tool === tool ? { ...a, allowed: !a.allowed } : a),
  };
  if (next.level === p.level && !tool) return;
  A.perms = next;
  drawPerms();
  try {
    await invoke("set_app_permissions", { slug: app.slug, level: next.level, allowed: next.actions.filter(a => a.allowed).map(a => a.tool) });
    A.error = "";
  } catch (err) {
    A.perms = p;
    A.error = String(err);
  }
  if (S.view === "appperms") drawPerms();
}

