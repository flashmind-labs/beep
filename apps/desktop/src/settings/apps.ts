import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { AppPermissionsView } from "./app-permissions-view.tsx";
import { McpView } from "./mcp-view.tsx";
import { bar, S } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
import { esc, plural } from "../ui/format.ts";
import { h } from "../ui/dom.ts";
import { ICON } from "../ui/icons.ts";
import { invoke, listen } from "../ui/tauri.ts";

// ---------- Apps ----------

/// An app as `list_apps` sends it.
export interface App {
  slug: string;
  name: string;
  description: string;
  logo_url: string | null;
  connected: boolean;
  /// False when the server lacks what the app needs to connect.
  configured: boolean;
  /// How it signs in: on a page the server hands out, or through a
  /// redirect to this Mac.
  kind: "composio" | "fastmail";
}

/// One account of an app that connects several, such as Fastmail.
export interface AppAccount {
  id: string;
  /// Its email address, once the server has read it.
  address: string | null;
}

/// A remote MCP server the user added.
export interface McpServer {
  id: string;
  name: string;
  url: string;
  connected: boolean;
  /// Whether it signs in with OAuth.
  sign_in: boolean;
  tools: string[];
}

/// Every tool a server lists, and how many Beep works well with.
export interface McpTools {
  tools: { name: string; description: string; enabled: boolean; usable: boolean }[];
  limit: number;
}

export type Level = "ask" | "allow" | "read";

/// What Beep may do in one app.
export interface AppPermissions {
  level: Level;
  actions: { tool: string; label: string; allowed: boolean }[];
}

interface AppsState {
  apps: App[] | null;
  error: string;
  /// The app whose sign-in is open, and the timer that looks for it.
  connecting: string | null;
  poll: ReturnType<typeof setInterval> | undefined;
  /// The app whose permissions are open, and them once read.
  open: App | null;
  perms: AppPermissions | null;
  /// MCP servers added by URL: null until listed, the add form, and the one
  /// whose sign-in is open in the browser.
  servers: McpServer[] | null;
  adding: boolean;
  addError: string;
  waiting: string | null;
  /// A server id, "new" or `account:<id>` while that is busy.
  busy: string | null;
  /// The server whose tools are open, its tools once listed, the tool
  /// waiting on the warning about going past the limit, and whether
  /// Remove waits for a second click.
  server: McpServer | null;
  tools: McpTools | null;
  asking: string | null;
  unadd: boolean;
  /// Fastmail's accounts by app slug, the agent's default first; missing when
  /// the server is too old to list them.
  accounts: Record<string, AppAccount[]>;
  removing: string | null;
  /// Why the open app's accounts did not load.
  accountsError: string;
}

/// The Apps view: every app, the one being connected and the one whose
/// permissions are open.
export const A: AppsState = { apps: null, error: "", connecting: null, poll: undefined, open: null, perms: null,
  servers: null, adding: false, addError: "", waiting: null, busy: null,
  server: null, tools: null, asking: null, unadd: false,
  accounts: {}, removing: null, accountsError: "" };

/// A logo of the first letter of `name`.
const letterLogo = (name: string) => `<span class="dalogo">${esc(name.slice(0, 1).toUpperCase())}</span>`;
const SPIN = '<span class="dmuted"><span class="dspin"></span></span>';

/// Logos shipped with the app, for apps Composio has no logo for.
const BUNDLED_LOGOS: Record<string, string> = { fastmail: "fastmail.svg" };

function appLogo(app: App) {
  const bundled = BUNDLED_LOGOS[app.slug];
  if (bundled) return `<img class="dalogo" src="${bundled}" alt="">`;
  if (!app.logo_url) return letterLogo(app.name);
  return `<img class="dalogo" src="${esc(app.logo_url)}" alt="" data-letter="${esc(app.name.slice(0, 1).toUpperCase())}">`;
}
/// A logo that does not load becomes the app's first letter.
function logoFallbacks(root: ParentNode) {
  root.querySelectorAll<HTMLImageElement>("img.dalogo[data-letter]").forEach(img => img.addEventListener("error", () => img.replaceWith(h(`<span class="dalogo">${esc(img.dataset.letter)}</span>`)), { once: true }));
}

/// The top of a list in Apps: the error, or a spinner until `loaded`.
const listTop = (loaded: unknown) => A.error ? `<div class="derr dpad">${esc(A.error)}</div>`
  : loaded ? "" : '<div class="dmuted dpad"><span class="dspin"></span></div>';

/// Stops waiting for a browser sign-in.
const cancelSignIn = () => invoke("cancel_app_sign_in").catch(() => {});

// ---------- Where Apps draws ----------

/// A page Apps shows, as `S.view` names it.
export type AppsView = "apps" | "appperms" | "mcp";

/// The window's Settings sheet shows Apps in its right pane: `root` is the
/// box Apps draws into instead of the page, `view` what it shows there,
/// and `opened` tells Settings when Apps opens another page (an app or a
/// server, by `id`) so its breadcrumb follows.
interface AppsHost {
  root: HTMLElement;
  view: AppsView | null;
  opened(view: AppsView, id: string | null): void;
}
let host: AppsHost | null = null;

/// Draws Apps into `root` from now on, until `appsOut` or until `root`
/// leaves the page. The page's own view (`S.view`) is left alone.
export function appsIn(root: HTMLElement, opened: AppsHost["opened"]) {
  host = { root, view: null, opened };
}
export function appsOut() {
  if (host && mcpRoot && host.root.contains(mcpRoot.host)) disposeMcp();
  if (host && permsRoot && host.root.contains(permsRoot.host)) disposePerms();
  host = null;
}
const hosted = () => (host?.root.isConnected ? host : null);
const root = () => hosted()?.root ?? bar;
const q = <T extends Element = HTMLElement>(sel: string) => root().querySelector<T>(sel);
/// Whether Apps shows `view`, in Settings or on the page.
const viewIs = (view: AppsView) => {
  const at = hosted();
  return at ? at.view === view : S.view === view;
};
function showView(view: AppsView, id: string | null) {
  const at = hosted();
  if (!at) return void (S.view = view);
  at.view = view;
  at.opened(view, id);
}
/// Settings has its own back link and Esc.
const backHint = () => (hosted() ? "" : hints([["esc", "back"]]));

export function renderApps() {
  disposeMcp();
  disposePerms();
  showView("apps", null);
  A.open = null;
  // In Settings the breadcrumb names the page.
  const head = hosted() ? "" : `<div class="dhead"><span class="tt"><b>Apps</b><small>Connect once, then just ask from any chat</small></span></div>`;
  root().innerHTML = `${head}
    <div class="dlist dapps" id="alist"></div>${backHint()}`;
  q("#alist")!.addEventListener("click", appsClick);
  drawApps();
  loadApps();
}

export async function loadApps() {
  try {
    A.apps = await invoke<App[]>("list_apps");
    A.error = "";
  } catch (e) {
    A.error = String(e);
  }

  // A server too old to know them leaves the section out.
  const [servers] = await Promise.all([
    invoke<McpServer[]>("list_mcp_servers").catch(() => A.servers),
    ...(A.apps ?? []).filter(a => a.kind === "fastmail").map(loadAccounts),
  ]);
  A.servers = servers;
  if (A.waiting && A.servers?.find(m => m.id === A.waiting)?.connected) A.waiting = null;
  // A Composio sign-in ends on the web page; the app shows up connected here.
  if (A.connecting && A.apps?.find(a => a.slug === A.connecting)?.connected) stopConnecting();
  if (viewIs("apps")) drawApps();
}

async function loadAccounts(app: App) {
  const accounts = app.connected ? await invoke<AppAccount[]>("list_app_accounts", { slug: app.slug }).catch(() => null) : [];
  if (accounts) A.accounts[app.slug] = accounts;
  else delete A.accounts[app.slug];
}

/// The addresses of a connected app's accounts, for its row in Apps.
function accountList(app: App) {
  const accounts = A.accounts[app.slug];
  return accounts?.length ? accounts.map(a => a.address ?? `${app.name} account`).join(", ") : null;
}

function appRow(app: App) {
  const end = A.connecting === app.slug ? waitingFor("data-cancel")
    : app.connected ? `<span class="dtag">Connected</span><span class="dmuted">${ICON.chev}</span>`
    : !app.configured ? '<span class="dmuted">Unavailable</span>'
    : `<button class="dbtn pri" data-connect="${esc(app.slug)}"${A.connecting ? " disabled" : ""}>Connect</button>`;
  return `<div class="dappr${app.connected ? " on" : ""}" ${app.connected ? `data-open="${esc(app.slug)}" role="button" tabindex="0"` : ""}>
      ${appLogo(app)}<span class="tt"><b>${esc(app.name)}</b><small>${esc((app.connected && accountList(app)) || app.description)}</small></span>${end}</div>`;
}

export function drawApps() {
  const list = q("#alist");
  if (!list) return;
  const apps = A.apps ?? [];
  const connected = apps.filter(a => a.connected), more = apps.filter(a => !a.connected);
  let html = listTop(A.apps);
  if (connected.length) html += '<div class="dsec">Connected</div>' + connected.map(appRow).join("");
  if (more.length) html += `<div class="dsec">${connected.length ? "More apps" : "Available"}</div>` + more.map(appRow).join("");
  if (A.servers) html += drawServers(A.servers);
  // Redrawing keeps what is typed in the add form.
  const fields = ["#mname", "#murl"], typed = fields.map(id => q<HTMLInputElement>(id)?.value), focus = document.activeElement?.id;
  list.innerHTML = html;
  logoFallbacks(list);
  fields.forEach((id, i) => {
    const input = q<HTMLInputElement>(id), value = typed[i];
    if (value != null && input) input.value = value;
  });
  if (focus) q("#" + focus)?.focus();
}

export function hostOf(url: string) {
  try { return new URL(url).host; } catch { return url; }
}

/// A sign-in open in the browser, with the Cancel that `attr` marks.
const waitingFor = (attr: string) => `<span class="dmuted"><span class="dspin"></span> Waiting for browser…</span><button class="dbtn quiet" ${attr}>Cancel</button>`;

/// The servers the user added, then the add form or its button.
function drawServers(servers: McpServer[]) {
  const row = (m: McpServer) => {
    const end = A.waiting === m.id ? waitingFor("data-mcancel")
      : A.busy === m.id ? SPIN
      : m.connected ? `<span class="dtag ok">${plural(m.tools.length, "tool", "tools")}</span><span class="dmuted">${ICON.chev}</span>`
      : `<span class="dtag ask">${m.sign_in ? "Sign-in expired" : "Not connected"}</span><button class="dbtn pri" data-mconnect="${esc(m.id)}">${m.sign_in ? "Sign in" : "Retry"}</button>`
        + `<button class="dbtn quiet warn" data-mremove="${esc(m.id)}">Remove</button>`;
    const open = m.connected && A.waiting !== m.id && A.busy !== m.id ? ` data-mopen="${esc(m.id)}" role="button" tabindex="0"` : "";
    return `<div class="dappr${open ? " on" : ""}"${open}>${letterLogo(m.name)}<span class="tt"><b>${esc(m.name)}</b><small>${esc(hostOf(m.url))}</small></span>${end}</div>`;
  };
  let html = '<div class="dsec">Your MCP servers</div>' + servers.map(row).join("");

  if (A.adding) {
    const busy = A.busy === "new", off = busy ? " disabled" : "";
    html += `<div class="dform dmcp">
      <label class="dfield"><span>Name</span><input id="mname" maxlength="60" placeholder="Team notes" autocomplete="off" spellcheck="false"${off}></label>
      <label class="dfield${A.addError ? " err" : ""}"><span>${A.addError ? esc(A.addError) : "Server URL"}</span><input id="murl" class="url" maxlength="2000" placeholder="https://example.com/mcp" autocomplete="off" spellcheck="false"${off}></label>
      <div class="dline">${busy ? '<span class="dmuted"><span class="dspin"></span> Connecting…</span>'
        : `<button class="dbtn pri" data-madd>Add server<kbd>${ICON.ret}</kbd></button><button class="dbtn quiet" data-mclose>Cancel</button>`}</div></div>`;
  } else {
    html += `<div class="dpad"><button class="dbtn" data-mnew>Add MCP server</button></div>`;
  }

  return html + '<div class="dmuted dpad dblock">Beep asks before every tool on a server you add.</div>';
}

export async function addServer() {
  const name = q<HTMLInputElement>("#mname")?.value.trim(), url = q<HTMLInputElement>("#murl")?.value.trim();
  if (!name || !url) return (name ? q("#murl") : q("#mname"))?.focus();
  A.busy = "new";
  A.addError = "";
  drawApps();

  try {
    const server = await invoke<McpServer>("add_mcp_server", { name, url });
    A.adding = false;
    if (!server.connected) A.waiting = server.id;
  } catch (e) {
    A.addError = String(e);
  }

  A.busy = null;
  await loadApps();
  if (A.adding) q("#murl")?.focus();
}

async function serverAction(kind: "connect" | "remove", id: string) {
  A.busy = id;
  A.error = "";
  drawApps();

  try {
    if (kind === "connect") {
      const server = await invoke<McpServer>("connect_mcp_server", { id });
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
listen<string | null>("mcp-signed-in", e => {
  A.waiting = null;
  if (e.payload) A.error = `Sign-in did not finish: ${e.payload}`;
  if (viewIs("apps")) loadApps();
});

async function appsClick(e: MouseEvent) {
  const target = e.target as HTMLElement;
  const d = target.closest<HTMLElement>("[data-mnew],[data-mclose],[data-madd],[data-mcancel],[data-mconnect],[data-mremove],[data-mopen]")?.dataset;

  if (d) {
    if (d.mnew != null) { A.adding = true; A.addError = ""; drawApps(); return q("#mname")?.focus(); }
    if (d.mclose != null) { A.adding = false; A.addError = ""; return drawApps(); }
    if (d.madd != null) return addServer();
    if (d.mcancel != null) { A.waiting = null; return drawApps(); }
    if (d.mconnect) return serverAction("connect", d.mconnect);
    if (d.mremove) return serverAction("remove", d.mremove);
    if (d.mopen) return renderMcpServer(A.servers!.find(s => s.id === d.mopen));
  }

  if (target.closest("[data-cancel]")) {
    cancelSignIn();
    return stopConnecting();
  }

  const connect = target.closest<HTMLElement>("[data-connect]");
  if (connect) return connectApp(A.apps!.find(a => a.slug === connect.dataset.connect));
  const open = target.closest<HTMLElement>("[data-open]");
  if (open) return renderAppPermissions(A.apps!.find(a => a.slug === open.dataset.open));
}

/// Signs in to `app` through the browser, with `add` to add an account to
/// it. The error, unless the user cancelled, is kept on the view.
async function signIn(app: App, add: boolean) {
  try {
    await invoke("connect_app", { slug: app.slug, kind: app.kind, add });
    return true;
  } catch (e) {
    if (A.connecting === app.slug && String(e) !== "cancelled") A.error = String(e);
    return false;
  }
}

async function connectApp(app: App | undefined) {
  if (!app || A.connecting) return;
  A.connecting = app.slug;
  A.error = "";
  drawApps();
  if (!await signIn(app, false)) return stopConnecting();
  // Fastmail is connected once the call returns. A Composio app finishes on
  // its web page, so look again until it shows up, for ten minutes at most.
  if (app.kind === "fastmail") return stopConnecting();
  const until = Date.now() + 10 * 60e3;
  A.poll = setInterval(() => {
    if (Date.now() > until || !viewIs("apps")) return stopConnecting();
    loadApps();
  }, 3000);
}

function stopConnecting() {
  clearInterval(A.poll);
  A.poll = undefined;
  A.connecting = null;
  loadApps();
}

// ---------- An MCP server's tools ----------

/// Opened from a connected server in Apps: its tools, each switched on or
/// off, and Remove.
let mcpRoot: { host: HTMLElement; root: Root; observer: MutationObserver } | null = null;
let mcpBusy = false;
function disposeMcp() {
  const current = mcpRoot;
  mcpRoot = null;
  if (!current) return;
  current.observer.disconnect();
  current.root.unmount();
}
export async function renderMcpServer(server: McpServer | undefined) {
  if (!server) return;
  disposeMcp();
  disposePerms();
  showView("mcp", server.id);
  Object.assign(A, { server, tools: null, asking: null, unadd: false, error: "" });
  mcpBusy = false;
  root().innerHTML = `<div id="mcp-react" style="display:contents"></div>${backHint()}`;
  const element = q<HTMLElement>("#mcp-react");
  if (!element) return;
  const reactRoot = createRoot(element);
  const observer = new MutationObserver(() => { if (mcpRoot?.host === element && !element.isConnected) disposeMcp(); });
  observer.observe(document.body, { childList: true, subtree: true });
  mcpRoot = { host: element, root: reactRoot, observer };
  const owner = mcpRoot;
  drawMcpTools();
  try {
    const tools = await invoke<McpTools>("list_mcp_tools", { id: server.id });
    if (mcpRoot === owner && viewIs("mcp") && A.server === server) A.tools = tools;
  } catch (error) {
    if (mcpRoot === owner && viewIs("mcp") && A.server === server) A.error = String(error);
  }
  if (mcpRoot === owner && viewIs("mcp") && A.server === server) drawMcpTools();
}
export function drawMcpTools() {
  const server = A.server, current = mcpRoot;
  if (!server || !current?.host.isConnected || !viewIs("mcp")) return;
  const toggle = (name: string) => {
    const tools = A.tools, tool = tools?.tools.find(item => item.name === name);
    if (!tools || !tool || mcpBusy) return;
    if (!tool.enabled && tools.tools.filter(item => item.enabled).length >= tools.limit) { A.asking = name; drawMcpTools(); }
    else void setTool(name, !tool.enabled);
  };
  flushSync(() => current.root.render(createElement(McpView, { server, tools: A.tools, error: A.error, asking: A.asking, confirming: A.unadd, busy: mcpBusy, toggle,
    confirmTool: () => { if (A.asking) void setTool(A.asking, true); },
    cancelTool: () => { A.asking = null; drawMcpTools(); },
    confirmRemove: () => { A.unadd = true; drawMcpTools(); },
    cancelRemove: () => { A.unadd = false; drawMcpTools(); },
    remove: () => void removeServer(server),
  })));
}
async function setTool(name: string, enabled: boolean) {
  const server = A.server;
  if (!server || mcpBusy) return;
  const owner = mcpRoot;
  mcpBusy = true; A.asking = null; drawMcpTools();
  try {
    const tools = await invoke<McpTools>("set_mcp_tool", { id: server.id, tool: name, enabled });
    if (mcpRoot === owner && viewIs("mcp") && A.server === server) { A.tools = tools; A.error = ""; }
  } catch (error) { if (mcpRoot === owner && viewIs("mcp") && A.server === server) A.error = String(error); }
  if (mcpRoot === owner && viewIs("mcp") && A.server === server) { mcpBusy = false; drawMcpTools(); }
}
async function removeServer(server: McpServer) {
  if (mcpBusy) return;
  const owner = mcpRoot;
  mcpBusy = true; drawMcpTools();
  try {
    await invoke("remove_mcp_server", { id: server.id });
    if (mcpRoot === owner && viewIs("mcp") && A.server === server) renderApps();
  } catch (error) {
    if (mcpRoot === owner && viewIs("mcp") && A.server === server) { A.error = String(error); mcpBusy = false; drawMcpTools(); }
  }
}

// ---------- An app's permissions ----------

let permsRoot: { host: HTMLElement; root: Root; observer: MutationObserver } | null = null;
let permsBusy = false;
const permsPending = new Set<string>();
const permsVersions = new Map<string, number>();
let accountSignInVersion = 0;
function disposePerms() {
  const current = permsRoot;
  permsRoot = null;
  if (!current) return;
  current.observer.disconnect();
  current.root.unmount();
}
const permsOn = (app: App, owner: typeof permsRoot) => !!owner && permsRoot === owner && viewIs("appperms") && A.open === app;
export async function renderAppPermissions(app: App | undefined) {
  if (!app) return;
  disposeMcp(); disposePerms();
  showView("appperms", app.slug);
  Object.assign(A, { open: app, perms: null, removing: null, error: "", accountsError: "" });
  permsBusy = false;
  root().innerHTML = `<div id="app-permissions-react" style="display:contents"></div>${backHint()}`;
  const element = q<HTMLElement>("#app-permissions-react");
  if (!element) return;
  const reactRoot = createRoot(element);
  const observer = new MutationObserver(() => { if (permsRoot?.host === element && !element.isConnected) disposePerms(); });
  observer.observe(document.body, { childList: true, subtree: true });
  permsRoot = { host: element, root: reactRoot, observer };
  const owner = permsRoot, version = permsVersions.get(app.slug) ?? 0;
  drawPerms();
  if (app.kind === "fastmail") void loadPageAccounts(app, owner);
  try {
    const permissions = await invoke<AppPermissions>("app_permissions", { slug: app.slug });
    if (permsOn(app, owner) && (permsVersions.get(app.slug) ?? 0) === version) A.perms = permissions;
  } catch (error) { if (permsOn(app, owner) && (permsVersions.get(app.slug) ?? 0) === version) A.error = String(error); }
  if (permsOn(app, owner)) drawPerms();
}
async function loadPageAccounts(app: App, owner: typeof permsRoot) {
  try {
    const accounts = await invoke<AppAccount[]>("list_app_accounts", { slug: app.slug });
    if (permsOn(app, owner)) { A.accounts[app.slug] = accounts; A.accountsError = ""; }
  } catch (error) { if (permsOn(app, owner)) A.accountsError = String(error); }
  if (permsOn(app, owner)) drawPerms();
}
async function addAccount(app: App, owner: typeof permsRoot) {
  if (A.connecting) return;
  const version = ++accountSignInVersion;
  A.connecting = app.slug; A.error = ""; drawPerms();
  try { await invoke("connect_app", { slug: app.slug, kind: app.kind, add: true }); }
  catch (error) { if (version === accountSignInVersion && permsOn(app, owner) && String(error) !== "cancelled") A.error = String(error); }
  if (version !== accountSignInVersion) return;
  A.connecting = null;
  if (permsOn(app, owner)) await loadPageAccounts(app, owner);
  else await loadAccounts(app);
}
async function removeAccount(app: App, id: string, owner: typeof permsRoot) {
  if (A.busy) return;
  A.busy = `account:${id}`; A.removing = null; drawPerms();
  let removed = false;
  try { await invoke("remove_app_account", { slug: app.slug, id }); removed = true; if (permsOn(app, owner)) A.error = ""; }
  catch (error) { if (permsOn(app, owner)) A.error = String(error); }
  A.busy = null;
  if (!permsOn(app, owner)) return;
  await loadPageAccounts(app, owner);
  if (!permsOn(app, owner)) return;
  if (removed && A.accounts[app.slug]?.length === 0) { A.apps = null; renderApps(); }
  else drawPerms();
}
function drawPerms() {
  const app = A.open, owner = permsRoot;
  if (!app || !owner?.host.isConnected || !permsOn(app, owner)) return;
  flushSync(() => owner.root.render(createElement(AppPermissionsView, {
    app, permissions: A.perms, error: A.error, accountsError: A.accountsError, accounts: A.accounts[app.slug] ?? null,
    connecting: A.connecting === app.slug, busy: A.busy, removing: A.removing, saving: permsBusy || permsPending.has(app.slug),
    retry: () => void renderAppPermissions(app), addAccount: () => void addAccount(app, owner),
    cancelSignIn: () => { accountSignInVersion++; cancelSignIn(); A.connecting = null; drawPerms(); },
    confirmAccount: (id: string) => { A.removing = id; drawPerms(); },
    cancelAccount: () => { A.removing = null; drawPerms(); },
    removeAccount: (id: string) => void removeAccount(app, id, owner),
    disconnect: () => void disconnectApp(app, owner),
    setLevel: (level: Level) => void savePermissions(app, owner, level, null),
    toggleAction: (tool: string) => void savePermissions(app, owner, null, tool),
  })));
}
async function disconnectApp(app: App, owner: typeof permsRoot) {
  if (permsBusy || permsPending.has(app.slug)) return;
  permsBusy = true; drawPerms();
  try {
    await invoke("disconnect_app", { slug: app.slug });
    if (permsOn(app, owner)) { A.apps = null; renderApps(); }
  } catch (error) { if (permsOn(app, owner)) { A.error = String(error); permsBusy = false; drawPerms(); } }
}
async function savePermissions(app: App, owner: typeof permsRoot, level: Level | null, tool: string | null) {
  const previous = A.perms;
  if (!previous || permsBusy || permsPending.has(app.slug) || (level === previous.level && !tool)) return;
  const next: AppPermissions = { level: level ?? previous.level, actions: previous.actions.map(action => action.tool === tool ? { ...action, allowed: !action.allowed } : action) };
  permsPending.add(app.slug);
  permsVersions.set(app.slug, (permsVersions.get(app.slug) ?? 0) + 1);
  A.perms = next; drawPerms();
  let failure: unknown = null;
  try { await invoke("set_app_permissions", { slug: app.slug, level: next.level, allowed: next.actions.filter(action => action.allowed).map(action => action.tool) }); }
  catch (error) { failure = error; }
  permsVersions.set(app.slug, (permsVersions.get(app.slug) ?? 0) + 1);
  if (permsOn(app, owner)) {
    if (failure) { A.perms = previous; A.error = String(failure); } else A.error = "";
  } else {
    const current = permsRoot, open = A.open;
    if (current && open?.slug === app.slug && permsOn(open, current)) {
      try {
        const permissions = await invoke<AppPermissions>("app_permissions", { slug: app.slug });
        if (permsOn(open, current)) { A.perms = permissions; A.error = failure ? String(failure) : ""; }
      } catch (error) { if (permsOn(open, current)) { A.perms = null; A.error = String(error); } }
    }
  }
  permsPending.delete(app.slug);
  if (A.open?.slug === app.slug && permsOn(A.open, permsRoot)) drawPerms();
}
