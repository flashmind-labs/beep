import { createElement, type ReactElement } from "react";
import { flushSync } from "react-dom";
import { openBarPage } from "../bar/page-root.ts";
import { AppBarFrame, AppsListView } from "./apps-list-view.tsx";
import { AppPermissionsView } from "./app-permissions-view.tsx";
import { McpView } from "./mcp-view.tsx";
import type { SettingsContentOwner } from "./settings.ts";
import { bar, S } from "../bar/core.ts";
import { hints } from "../bar/hints.ts";
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
  serverName: string;
  serverUrl: string;
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
  servers: null, adding: false, serverName: "", serverUrl: "", addError: "", waiting: null, busy: null,
  server: null, tools: null, asking: null, unadd: false,
  accounts: {}, removing: null, accountsError: "" };

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
  owner: SettingsContentOwner;
  view: AppsView | null;
  opened(view: AppsView, id: string | null): void;
}
let host: AppsHost | null = null;

/// Draws Apps into `root` from now on, until `appsOut` or until `root`
/// leaves the page. The page's own view (`S.view`) is left alone.
export function appsIn(root: HTMLElement, opened: AppsHost["opened"], owner: SettingsContentOwner) {
  host = { root, owner, view: null, opened };
}
export function appsOut() {
  if (host && appsRoot && host.root.contains(appsRoot.host)) disposeApps();
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

interface AppMount {
  host: HTMLElement;
  render(content: ReactElement): void;
  unmount(): void;
  observer: MutationObserver;
}
function mountView(section: AppsView, detached: () => void): AppMount {
  const embedded = hosted();
  const page = embedded ? null : openBarPage(() => { disposeApps(); disposeMcp(); disposePerms(); });
  const element = page?.host ?? document.createElement("div");
  if (embedded) {
    element.style.display = "contents";
    element.id = section === "apps" ? "apps-react" : section === "mcp" ? "mcp-react" : "app-permissions-react";
    embedded.root.replaceChildren(element);
  }
  const observer = new MutationObserver(() => { if (!element.isConnected) detached(); });
  observer.observe(document.body, { childList: true, subtree: true });
  return {
    host: element, observer,
    render(content) {
      if (embedded) embedded.owner.show(section, element, content);
      else if (page) flushSync(() => page.root.render(createElement(AppBarFrame, { section, hints: backHint(), children: content })));
    },
    unmount() {
      if (embedded) embedded.owner.remove(section);
      else if (page) flushSync(() => page.root.render(null));
    },
  };
}

let appsRoot: AppMount | null = null;
function disposeApps() {
  const current = appsRoot;
  appsRoot = null;
  if (!current) return;
  current.observer.disconnect();
  current.unmount();
}
export function renderApps() {
  disposeMcp(); disposePerms(); disposeApps();
  showView("apps", null);
  A.open = null;
  appsRoot = mountView("apps", () => { disposeApps(); });
  drawApps();
  void loadApps();
}

let appsReadVersion = 0;
export async function loadApps() {
  const version = ++appsReadVersion;
  try {
    const apps = await invoke<App[]>("list_apps");
    if (version !== appsReadVersion) return;
    A.apps = apps;
    A.error = "";
  } catch (error) {
    if (version !== appsReadVersion) return;
    A.error = String(error);
  }

  const fastmail = (A.apps ?? []).filter(app => app.kind === "fastmail");
  const [servers, ...accounts] = await Promise.all([
    invoke<McpServer[] | null>("list_mcp_servers").catch(() => A.servers),
    ...fastmail.map(app => app.connected ? invoke<AppAccount[] | null>("list_app_accounts", { slug: app.slug }).catch(() => null) : Promise.resolve([])),
  ]);
  if (version !== appsReadVersion) return;
  A.servers = servers;
  fastmail.forEach((app, index) => { const rows = accounts[index]; if (rows) A.accounts[app.slug] = rows; else delete A.accounts[app.slug]; });
  if (A.waiting && A.servers?.find(server => server.id === A.waiting)?.connected) A.waiting = null;
  if (A.connecting && A.apps?.find(app => app.slug === A.connecting)?.connected) stopConnecting();
  if (viewIs("apps")) drawApps();
}

async function loadAccounts(app: App) {
  const accounts = app.connected ? await invoke<AppAccount[]>("list_app_accounts", { slug: app.slug }).catch(() => null) : [];
  if (accounts) A.accounts[app.slug] = accounts;
  else delete A.accounts[app.slug];
}

export function drawApps() {
  const current = appsRoot;
  if (!current?.host.isConnected || !viewIs("apps")) return;
  current.render(createElement(AppsListView, {
    apps: A.apps, accounts: A.accounts, error: A.error, connecting: A.connecting, servers: A.servers,
    waiting: A.waiting, busy: A.busy, adding: A.adding, addError: A.addError, serverName: A.serverName, serverUrl: A.serverUrl,
    changeName: (value: string) => { A.serverName = value; }, changeUrl: (value: string) => { A.serverUrl = value; },
    add: () => void addServer(), startAdd: () => { addFormVersion++; A.adding = true; A.serverName = ""; A.serverUrl = ""; A.addError = ""; drawApps(); },
    cancelAdd: cancelServerAdd,
    cancelServer: () => { A.waiting = null; drawApps(); },
    connectServer: (id: string) => void serverAction("connect", id), removeServer: (id: string) => void serverAction("remove", id),
    openServer: (id: string) => void renderMcpServer(A.servers?.find(server => server.id === id)),
    connectApp: (slug: string) => void connectApp(A.apps?.find(app => app.slug === slug)),
    openApp: (slug: string) => void renderAppPermissions(A.apps?.find(app => app.slug === slug)),
    cancelApp: () => { cancelSignIn(); stopConnecting(); },
  }));
}
export function hostOf(url: string) {
  try { return new URL(url).host; } catch { return url; }
}

let addFormVersion = 0;
export function cancelServerAdd() { addFormVersion++; A.adding = false; A.addError = ""; drawApps(); }
export async function addServer() {
  if (A.busy) return;
  const version = addFormVersion;
  const name = q<HTMLInputElement>("#mname")?.value.trim(), url = q<HTMLInputElement>("#murl")?.value.trim();
  if (!name || !url) return (name ? q("#murl") : q("#mname"))?.focus();
  A.busy = "new";
  A.addError = "";
  drawApps();

  try {
    const server = await invoke<McpServer>("add_mcp_server", { name, url });
    if (addFormVersion === version) { A.adding = false; A.serverName = ""; A.serverUrl = ""; }
    if (!server.connected) A.waiting = server.id;
  } catch (e) {
    if (addFormVersion === version) A.addError = String(e);
  }

  A.busy = null;
  await loadApps();
  if (A.adding && addFormVersion === version) q("#murl")?.focus();
}

async function serverAction(kind: "connect" | "remove", id: string) {
  if (A.busy) return;
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

/// Signs in to `app` through the browser, with `add` to add an account to
/// it. The error, unless the user cancelled, is kept on the view.
let appConnectionVersion = 0;
async function signIn(app: App, add: boolean, version: number) {
  try {
    await invoke("connect_app", { slug: app.slug, kind: app.kind, add });
    return true;
  } catch (e) {
    if (version === appConnectionVersion && A.connecting === app.slug && String(e) !== "cancelled") A.error = String(e);
    return false;
  }
}

async function connectApp(app: App | undefined) {
  if (!app || A.connecting) return;
  const version = ++appConnectionVersion;
  A.connecting = app.slug;
  A.error = "";
  drawApps();
  const signedIn = await signIn(app, false, version);
  if (version !== appConnectionVersion) return;
  if (!signedIn) return stopConnecting();
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
  appConnectionVersion++;
  clearInterval(A.poll);
  A.poll = undefined;
  A.connecting = null;
  loadApps();
}

// ---------- An MCP server's tools ----------

/// Opened from a connected server in Apps: its tools, each switched on or
/// off, and Remove.
let mcpRoot: AppMount | null = null;
let mcpBusy = false;
function disposeMcp() {
  const current = mcpRoot;
  mcpRoot = null;
  if (!current) return;
  current.observer.disconnect();
  current.unmount();
}
export async function renderMcpServer(server: McpServer | undefined) {
  if (!server) return;
  disposeMcp();
  disposePerms();
  disposeApps();
  showView("mcp", server.id);
  Object.assign(A, { server, tools: null, asking: null, unadd: false, error: "" });
  mcpBusy = false;
  mcpRoot = mountView("mcp", () => { disposeMcp(); });
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
  current.render(createElement(McpView, { server, tools: A.tools, error: A.error, asking: A.asking, confirming: A.unadd, busy: mcpBusy, toggle,
    confirmTool: () => { if (A.asking) void setTool(A.asking, true); },
    cancelTool: () => { A.asking = null; drawMcpTools(); },
    confirmRemove: () => { A.unadd = true; drawMcpTools(); },
    cancelRemove: () => { A.unadd = false; drawMcpTools(); },
    remove: () => void removeServer(server),
  }));
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

let permsRoot: AppMount | null = null;
let permsBusy = false;
const permsPending = new Set<string>();
const permsVersions = new Map<string, number>();
let accountSignInVersion = 0;
function disposePerms() {
  const current = permsRoot;
  permsRoot = null;
  if (!current) return;
  current.observer.disconnect();
  current.unmount();
}
const permsOn = (app: App, owner: typeof permsRoot) => !!owner && permsRoot === owner && viewIs("appperms") && A.open === app;
export async function renderAppPermissions(app: App | undefined) {
  if (!app) return;
  disposeMcp(); disposePerms(); disposeApps();
  showView("appperms", app.slug);
  Object.assign(A, { open: app, perms: null, removing: null, error: "", accountsError: "" });
  permsBusy = false;
  permsRoot = mountView("appperms", () => { disposePerms(); });
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
  owner.render(createElement(AppPermissionsView, {
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
  }));
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
