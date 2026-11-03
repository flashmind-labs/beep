// Settings, Credentials: git hosts (GitHub, one row per Forgejo server) then
// one API key per provider.

type Esc = (s: unknown) => string;
/// "today", "3 days ago": how long ago a time in milliseconds was.
type Ago = (ms: number) => string;

/// A saved credential (CredentialView), with what its kind adds.
export type Credential = {
  id: string;
  last4: string;
  account: string | null;
  env_var: string;
  to_machines: boolean;
  /// Milliseconds since the Unix epoch.
  created_at: number;
  /// The server can no longer open it: the row asks for it again.
  unreadable?: boolean;
} & ({ kind: "github" } | { kind: "forgejo"; url: string } | { kind: "api_key"; provider: string });

type Forgejo = Extract<Credential, { kind: "forgejo" }>;

/// One GitHub account (GitHubAccountView).
export interface GitHubAccount {
  id: string;
  login: string;
  orgs: string[];
  default: boolean;
  last4: string;
  created_at: number;
  unreadable: boolean;
}

/// A provider an API key can be added for (ProviderView).
export interface Provider {
  slug: string;
  name: string;
  env_var: string;
}

/// A CLI's sign-in on this Mac (LoginView).
export type CliLogin = { state: "signed_in"; account?: string | null } | { state: "signed_out" } | { state: "missing" };

/// What gh and teacli on this Mac are signed in to (CliLogins).
export interface CliLogins {
  github?: CliLogin;
  github_accounts?: { login: string; active?: boolean }[];
  forgejo?: { url: string }[];
}

/// gh's browser sign-in under way: the code to enter and where.
export interface GhFlow {
  code: string;
  url: string;
  copied?: boolean;
}

/// Checking a saved Forgejo token (CheckView): from the server, else from
/// a machine with teacli. `checking` while it runs.
export type CredCheck =
  | { state: "checking" }
  | { state: "valid"; account: string; machine?: string | null }
  | { state: "refused"; machine?: string | null; message: string }
  | { state: "unreachable"; message: string };

/// The row whose token is being typed, as a `data-cred-edit` value names it.
export type CredEdit =
  | { kind: "github"; id?: string }
  | { kind: "forgejo"; id?: string; url?: string }
  | { kind: "api_key"; provider: string; toMachines: boolean };

/// What `save_credential` takes (NewCredential).
export type NewCredential =
  | { kind: "github"; secret: string; to_machines: boolean }
  | { kind: "forgejo"; url: string; secret: string; to_machines: boolean }
  | { kind: "api_key"; provider: string; secret: string; to_machines: boolean };

/// The section's state.
export interface CredState {
  list?: Credential[] | null;
  github?: GitHubAccount[];
  providers?: Provider[];
  edit?: CredEdit | null;
  busy?: boolean;
  error?: string;
  cli?: CliLogins | null;
  cliBusy?: string;
  cliError?: { key: string; message: string } | null;
  ghFlow?: GhFlow | null;
  ghAdd?: boolean;
  /// Checks of Forgejo tokens the server could not check when saved, by id.
  checks?: Record<string, CredCheck>;
}

/// A machine as far as the tools it reports.
export interface ToolMachine {
  name: string;
  env?: { tools?: { name: string }[] } | null;
}

/// `git.example.com` from `https://git.example.com/`.
export function hostOf(url: string | null | undefined) {
  return String(url || "").replace(/^[a-z]+:\/\//i, "").replace(/\/+$/, "");
}

/// The names of the machines that report `tool`, or null when the machines
/// are not known (the list could not be read).
export function machinesWith(machines: ToolMachine[] | null, tool: string) {
  if (!Array.isArray(machines)) return null;
  return machines.filter(m => m.env?.tools?.some(t => t.name === tool)).map(m => m.name);
}

/// Where a CLI is, as one short chip: "gh on build-box" or "gh on 3 machines"
/// with their names as `title`, or null when the machines are not known.
export function toolChip(cli: string, machines: ToolMachine[] | null) {
  const on = machinesWith(machines, cli);
  if (on === null) return null;
  if (!on.length) return { text: `${cli} on no machine yet`, title: "" };
  return { text: on.length === 1 ? `${cli} on ${on[0]}` : `${cli} on ${on.length} machines`, title: on.join(", ") };
}

/// The orgs shown as chips: the first `max`, and the rest folded into one
/// "+N" chip.
export function foldOrgs(orgs: readonly string[] | null | undefined, max = 3) {
  const all = orgs ?? [];
  return all.length > max ? { shown: all.slice(0, max), more: all.slice(max) } : { shown: [...all], more: [] };
}

export type Draft = { error: string; credential?: undefined } | { credential: NewCredential; error?: undefined };

/// What Save sends for the row being edited, or an error to show.
export function draft(edit: CredEdit | null | undefined, url: string | undefined, secret: string | undefined, toMachines?: boolean): Draft {
  secret = String(secret || "").trim();
  if (!edit) return { error: "Nothing to save." };
  if (!secret) return { error: "Paste the token first." };
  if (edit.kind === "forgejo") {
    const u = String(url || edit.url || "").trim();
    if (!/^https?:\/\/[^\s/]+/i.test(u)) return { error: "The server URL starts with https://, such as https://git.example.com." };
    return { credential: { kind: "forgejo", url: u, secret, to_machines: true } };
  }
  if (edit.kind === "github") return { credential: { kind: "github", secret, to_machines: true } };
  return { credential: { kind: "api_key", provider: edit.provider, secret, to_machines: !!toMachines } };
}

/// GitHub's page for a new token with what gh and Beep need.
export const GITHUB_TOKEN_PAGE = "https://github.com/settings/tokens/new?scopes=repo,read:org,workflow&description=Beep";

/// Whether two server URLs name the same server (scheme, host and port).
export function sameServer(a: string | undefined, b: string | undefined) {
  try {
    const x = new URL(String(a).trim()), y = new URL(String(b).trim());
    return /^https?:$/.test(x.protocol) && x.origin === y.origin;
  } catch {
    return false;
  }
}

/// A Forgejo server's page for new access tokens, or null for a URL that
/// is not one.
export function forgejoTokenPage(url: string) {
  try {
    const u = new URL(String(url || "").trim());
    if (!/^https?:$/.test(u.protocol)) return null;
    return `${u.origin}${u.pathname.replace(/\/+$/, "")}/user/settings/applications`;
  } catch {
    return null;
  }
}

/// The servers teacli on this Mac has a token for that Beep does not.
export function teaOffers(cli: CliLogins | null | undefined, list: Credential[] | null | undefined) {
  const saved = (list ?? []).filter((c): c is Forgejo => c.kind === "forgejo");
  return (cli?.forgejo ?? []).filter(s => !saved.some(c => sameServer(c.url, s.url)));
}

/// Whether teacli on this Mac has a token for `url`.
export function teaHas(cli: CliLogins | null | undefined, url: string) {
  return (cli?.forgejo ?? []).some(s => sameServer(s.url, url));
}

/// The host a `data-cred-cli` value names, as `use_cli_login` takes it.
export function cliHost(key: string) {
  if (key === "github") return { kind: "github" };
  if (String(key).startsWith("forgejo:")) return { kind: "forgejo", url: key.slice("forgejo:".length) };
  return null;
}

const sm = 'style="font-size:11.5px;padding:2px 8px;min-height:24px"';

const GITHUB_ICON = `<svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor" aria-hidden="true"><path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"/></svg>`;
const FORGEJO_ICON = `<svg viewBox="0 0 212 212" width="15" height="15" aria-hidden="true"><g fill="none" transform="translate(6 6)"><path d="M58 168V70a50 50 0 0 1 50-50h20" stroke="#f60" stroke-width="25"/><path d="M58 168v-30a50 50 0 0 1 50-50h20" stroke="#d40000" stroke-width="25"/><circle cx="142" cy="20" r="18" stroke="#f60" stroke-width="15"/><circle cx="142" cy="88" r="18" stroke="#d40000" stroke-width="15"/><circle cx="58" cy="180" r="18" stroke="#d40000" stroke-width="15"/></g></svg>`;
const EDIT_ICON = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17 3a2.85 2.85 0 0 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>`;
const REMOVE_ICON = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/></svg>`;

/// What the inline form shows above and under its fields.
interface FormText {
  title: string;
  url?: string;
  note?: string;
  noteHtml?: string;
  env?: string;
}

/// The inline form for adding or replacing one credential.
function form(state: CredState, esc: Esc, { title, note, noteHtml, env, url }: FormText) {
  const e = state.edit!;
  const urlField = e.kind === "forgejo" && !e.id
    ? `<input id="credUrl" class="url" type="url" placeholder="https://git.example.com" autocomplete="off" spellcheck="false" value="${esc(e.url ?? "")}" aria-label="Server URL">`
    : "";
  const check = e.kind === "api_key" && env
    ? `<label style="display:flex;gap:8px;align-items:center;font-size:12px;color:var(--muted)"><input type="checkbox" id="credMachines"${e.toMachines !== false ? " checked" : ""}> Give it to my machines as ${esc(env)}</label>`
    : "";
  const label = e.kind === "api_key" && e.provider !== "openrouter" ? "Save" : "Check and save";
  return `<div class="dcred-form" style="padding:10px 0;border-bottom:1px solid var(--border);display:grid;gap:8px">
    <div style="display:flex;align-items:center;gap:8px;font-size:13px"><b style="font-weight:600">${esc(title)}</b>${url ? `<span style="color:var(--muted);font-size:12px">${esc(url)}</span>` : ""}</div>
    <div class="dfield${state.error ? " err" : ""}" style="gap:8px">${urlField}
      <div style="display:flex;gap:8px"><input id="credSecret" type="password" style="flex:1;font-family:var(--mono);font-size:12.5px" placeholder="Paste the token" autocomplete="off" spellcheck="false" aria-label="Token">
      <button class="dbtn pri" id="credSave"${state.busy ? " disabled" : ""}>${state.busy ? "Checking…" : label}</button><button class="dbtn quiet" id="credCancel">Cancel</button></div></div>
    ${check}${note || noteHtml ? `<span style="font-size:11.5px;color:var(--muted)">${noteHtml ?? esc(note)}</span>` : ""}
    <div class="derr" id="credErr">${esc(state.error || "")}</div></div>`;
}

/// The last 4 of a saved secret, or a prompt to enter it again when the
/// server can no longer open it.
function shown(esc: Esc, cred: { last4: string; unreadable?: boolean }, what: string) {
  return cred.unreadable
    ? `<span style="color:var(--err)">Re-enter this ${what}</span>`
    : `<code style="font-size:11px">…${esc(cred.last4)}</code>`;
}

/// A link that opens in the browser.
const link = (esc: Esc, href: string, text: string) => `<a href="${esc(href)}" data-cred-open="${esc(href)}" style="color:var(--accent)">${esc(text)}</a>`;
const code = (text: string) => `<code style="font-size:11px">${text}</code>`;
/// A muted line in a row.
const note = (html: string) => `<span style="font-size:11.5px;color:var(--muted)">${html}</span>`;
/// Why a sign-in was refused, under its row.
const rowError = (esc: Esc, error: string | undefined) => error ? `<span class="derr">${esc(error)}</span>` : "";

/// A short chip on a git host's second line. `kind` is `warn` for a
/// problem worth a look and `err` for one that stops the host working.
const chip = (esc: Esc, text: string, title = "", kind = "") =>
  `<span class="dgchip${kind ? ` ${kind}` : ""}"${title ? ` title="${esc(title)}"` : ""}>${esc(text)}</span>`;

/// The chip for where a CLI is, or "" when the machines are not known.
function cliChip(esc: Esc, cli: string, machines: ToolMachine[] | null) {
  const c = toolChip(cli, machines);
  return c ? chip(esc, c.text, c.title) : "";
}

/// A small icon button with its label as tooltip, carrying `attr`.
const iconButton = (esc: Esc, icon: string, label: string, attr: string) =>
  `<button class="dbtn quiet dico" title="${esc(label)}" aria-label="${esc(label)}" ${attr}>${icon}</button>`;
/// Replace and Remove for a saved host, as icon buttons.
const editRemove = (esc: Esc, what: string, editKey: string, id: string) =>
  iconButton(esc, EDIT_ICON, `Replace the token for ${what}`, `data-cred-edit="${esc(editKey)}"`)
  + iconButton(esc, REMOVE_ICON, `Remove ${what}`, `data-cred-remove="${esc(id)}"`);

/// "Use gh sign-in", or "Checking…" while it is being saved.
const useButton = (esc: Esc, key: string, label: string, busy: boolean) =>
  `<button class="dbtn pri" ${sm} data-cred-cli="${esc(key)}"${busy ? " disabled" : ""}>${busy ? "Checking…" : esc(label)}</button>`;

/// The code gh's browser sign-in waits for, large and selectable, or a
/// spinner until gh prints it.
function ghWaiting(esc: Esc, flow: GhFlow) {
  const entry = flow.code
    ? `<span>Enter code <code style="font-family:var(--mono);font-size:18px;font-weight:600;letter-spacing:.1em;user-select:all;-webkit-user-select:all;cursor:text">${esc(flow.code)}</code> on ${link(esc, flow.url, "GitHub")}${flow.copied ? `<span style="color:var(--muted)"> (copied)</span>` : ""}</span>`
    : "";
  return `<div style="padding:8px 10px;border-radius:8px;border:1px solid var(--border);font-size:12.5px;display:grid;gap:6px">${entry}
    <span style="display:flex;gap:6px;align-items:center;color:var(--muted)"><span class="dspin"></span>${flow.code ? "Waiting for GitHub" : "Starting gh…"}</span></div>`;
}

/// One git host's row.
interface GitRow {
  icon: string;
  name: string;
  /// Plain text, such as `@octo-one` or `gh on this Mac as @octo-one`.
  account?: string;
  /// Html, such as the default chip.
  tag?: string;
  /// Dims the icon when nothing usable is saved.
  usable: boolean;
  acts: string;
  /// Html chips for the second line.
  chips?: string;
  /// Html under the chips.
  more?: string;
}

function gitRow(esc: Esc, { icon, name, account, tag, usable, acts, chips, more }: GitRow) {
  return `<div class="dgit${usable ? "" : " off"}"><span class="ic">${icon}</span>
    <div class="l1"><b>${esc(name)}</b>${account ? `<span class="acct">${esc(account)}</span>` : ""}${tag ?? ""}<span class="acts">${acts}</span></div>
    ${chips ? `<div class="l2">${chips}</div>` : ""}${more ?? ""}</div>`;
}

/// What a git host row offers when nothing usable is saved: the account
/// line, the buttons, help under it and a body under that.
export interface Offer {
  account?: string;
  acts: string;
  help: string;
  body?: string;
}

/// What the GitHub row offers when no usable token is saved, from this Mac's
/// gh: its sign-in, signing it in, or a link to make a token.
export function ghOffer(esc: Esc, gh: CliLogin | null | undefined, busy: boolean, flow: GhFlow | null | undefined): Offer {
  const paste = (quiet: boolean) => `<button class="dbtn${quiet ? " quiet" : ""}" ${sm} data-cred-edit="github">Paste a token</button>`;
  if (flow) return { acts: `<button class="dbtn quiet" ${sm} data-cred-gh-cancel>Cancel</button>`, help: "", body: ghWaiting(esc, flow) };
  switch (gh?.state) {
    case "signed_in":
      return {
        account: gh.account ? `gh on this Mac as @${gh.account}` : "gh signed in on this Mac",
        acts: useButton(esc, "github", "Use gh sign-in", busy) + paste(true),
        help: "",
      };
    case "signed_out":
      return {
        acts: `<button class="dbtn pri" ${sm} data-cred-gh>Sign in with GitHub</button>${paste(true)}`,
        help: "Opens GitHub in your browser, then saves the sign-in here",
      };
    case "missing":
      return {
        acts: paste(true),
        help: `Make a token on ${link(esc, GITHUB_TOKEN_PAGE, "GitHub")}, then paste it.<br>Or install GitHub CLI (${code("brew install gh")}) and sign in here`,
      };
    default:
      return { acts: paste(false), help: "" };
  }
}

/// The sign-in being saved for one row, or why it was refused.
interface RowState {
  busy: boolean;
  error: string;
}

/// One git host row.
interface HostRow extends Partial<RowState> {
  icon: string;
  name: string;
  cred?: Credential;
  cli: string;
  machines: ToolMachine[] | null;
  editKey: string;
  offer?: Offer;
  cliKey?: string;
  check?: CredCheck;
}

/// One git host: icon, name and login, then where its CLI is.
function hostRow(esc: Esc, { icon, name, cred, cli, machines, editKey, offer, cliKey, busy, error, check }: HostRow) {
  const usable = !!cred && !cred.unreadable;
  const login = cred?.account || (check?.state === "valid" ? check.account : "");
  const account = usable ? (login ? `@${login}` : "") : offer?.account ?? "";
  const use = cliKey && !usable ? useButton(esc, cliKey, `Use ${cli} sign-in`, !!busy) : "";
  const acts = cred
    ? use + editRemove(esc, name, editKey, cred.id)
    : offer?.acts ?? `<button class="dbtn" ${sm} data-cred-edit="${esc(editKey)}">Add token</button>`;
  const chips = (cred?.unreadable ? chip(esc, "Re-enter this token", "", "err") : "")
    + (usable ? checkChip(esc, name, !!cred.account, check) : "")
    + cliChip(esc, cli, machines);
  const help = !usable && offer?.help ? note(offer.help) : "";
  const body = !usable && offer?.body ? offer.body : "";
  if (usable && check?.state === "refused") error = error || check.message;
  return gitRow(esc, { icon, name, account, usable, acts, chips, more: help + body + rowError(esc, error) });
}

/// What a saved Forgejo token's check says, as one chip: where it was checked
/// from, that it is being checked, or why it could not be.
export function checkChip(esc: Esc, host: string, known: boolean, check: CredCheck | undefined) {
  switch (check?.state) {
    case "checking":
      return chip(esc, "checking from your machines…");
    case "valid":
      return check.machine ? chip(esc, `checked from ${check.machine}`, `The server cannot reach ${host}; ${check.machine} can`) : "";
    case "refused":
      return chip(esc, "token refused", check.message, "err");
    case "unreachable":
      return chip(esc, "not reachable from the server or your machines", check.message, "warn");
    default:
      return known ? "" : chip(esc, "not checked: the server could not reach it", "", "warn");
  }
}

/// One provider's API key row.
function keyRow(esc: Esc, ago: Ago, p: Provider, cred: Credential | undefined) {
  const start = `<div style="display:flex;align-items:center;gap:8px;padding:9px 0;border-bottom:1px solid var(--border);font-size:13px">`;
  if (!cred) return `${start}
    <span class="ddot off"></span><b style="font-weight:600">${esc(p.name)}</b><code style="font-size:11px;color:var(--muted)">${esc(p.env_var)}</code>
    <span style="margin-left:auto"><button class="dbtn" ${sm} data-cred-edit="api:${esc(p.slug)}">Add key</button></span></div>`;
  const use = !cred.unreadable && cred.to_machines ? `your machines as ${esc(p.env_var)}` : "not given to your machines";
  return `${start}
    <span class="ddot"></span><span style="display:grid;gap:2px;min-width:0"><b style="font-weight:600">${esc(p.name)}</b><span style="font-size:11.5px;color:var(--muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${shown(esc, cred, "key")} · ${use} · added ${esc(ago(cred.created_at))}</span></span>
    <span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn" ${sm} data-cred-edit="api:${esc(p.slug)}">Replace</button><button class="dbtn quiet" ${sm} data-cred-remove="${esc(cred.id)}" aria-label="Remove the ${esc(p.name)} key">Remove</button></span></div>`;
}

/// The GitHub accounts: from `ListGitHubAccounts` when the server answered
/// it, else the one GitHub row of the credentials list, as the default.
export function githubAccounts(state: CredState): GitHubAccount[] {
  if (Array.isArray(state.github)) return state.github;
  const row = (state.list ?? []).find(c => c.kind === "github");
  return row ? [{ id: row.id, login: row.account ?? "", orgs: [], default: true, last4: row.last4, created_at: row.created_at, unreadable: !!row.unreadable }] : [];
}

const sameLogin = (a: string | undefined, b: string | undefined) => String(a ?? "").toLowerCase() === String(b ?? "").toLowerCase();

/// The accounts gh on this Mac has that Beep does not.
export function ghChoices(cli: CliLogins | null | undefined, accounts: GitHubAccount[]) {
  return (cli?.github_accounts ?? []).filter(g => !accounts.some(a => !a.unreadable && sameLogin(a.login, g.login)));
}

/// "Use" for one gh account, or "Checking…" while it is being saved.
const ghUse = (esc: Esc, login: string, busy: boolean) =>
  `<button class="dbtn pri" ${sm} data-cred-gh-use="${esc(login)}"${busy ? " disabled" : ""}>${busy ? "Checking…" : "Use"}</button>`;

/// The gh accounts on this Mac, one line each with Use.
function ghChoiceRows(esc: Esc, choices: { login: string; active?: boolean }[], cliBusy: string | undefined) {
  return choices.map(g => `<div style="display:flex;align-items:center;gap:8px;font-size:12.5px"><span style="color:var(--muted)">gh on this Mac as @${esc(g.login)}</span>${g.active ? `<span style="font-size:10.5px;color:var(--dim)">active</span>` : ""}<span style="margin-left:auto">${ghUse(esc, g.login, cliBusy === `gh:${g.login}`)}</span></div>`).join("");
}

/// What an account row needs besides the account.
interface AccountRowState extends RowState {
  machines: ToolMachine[] | null;
  cli: CliLogins | null | undefined;
}

/// One saved GitHub account: login and default tag, Make default and the
/// icon buttons; under it its orgs and where gh is.
function accountRow(esc: Esc, acc: GitHubAccount, { machines, cli, busy, error }: AccountRowState) {
  const usable = !acc.unreadable;
  const ghHere = (cli?.github_accounts ?? []).some(g => sameLogin(g.login, acc.login)) || cli?.github?.state === "signed_in";
  const use = !usable && ghHere ? useButton(esc, "github", "Use gh sign-in", busy) : "";
  const make = acc.default ? "" : `<button class="dbtn" ${sm} data-cred-gh-default="${esc(acc.id)}">Make default</button>`;
  const what = acc.login ? `@${acc.login}` : "GitHub";
  const { shown: orgs, more } = foldOrgs(acc.orgs);
  const chips = (usable ? "" : chip(esc, "Re-enter this token", "", "err"))
    + orgs.map(o => chip(esc, o)).join("")
    + (more.length ? chip(esc, `+${more.length}`, more.join(", ")) : "")
    + cliChip(esc, "gh", machines);
  return gitRow(esc, {
    icon: GITHUB_ICON, name: "GitHub", account: acc.login ? `@${acc.login}` : "", usable,
    tag: acc.default ? `<span class="dgchip" title="GH_TOKEN on your machines">default</span>` : "",
    acts: use + make + editRemove(esc, what, `github:${acc.id}`, acc.id),
    chips, more: rowError(esc, error),
  });
}

/// Adding another GitHub account: gh's accounts here, gh's browser
/// sign-in, which adds one to gh too, or a pasted token.
function addAccount(esc: Esc, state: CredState, accounts: GitHubAccount[]) {
  const cli = state.cli;
  const flow = state.ghFlow;
  const err = state.cliError?.key === "github" ? `<span class="derr">${esc(state.cliError.message)}</span>` : "";
  const top = `<div style="display:flex;align-items:center;gap:8px"><b style="font-weight:600">Add another GitHub account</b><span style="margin-left:auto">${flow ? `<button class="dbtn quiet" ${sm} data-cred-gh-cancel>Cancel</button>` : `<button class="dbtn quiet" ${sm} data-cred-gh-add="">Cancel</button>`}</span></div>`;
  const box = (body: string) => `<div style="padding:10px 0;border-bottom:1px solid var(--border);display:grid;gap:6px;font-size:13px">${top}<div style="display:grid;gap:6px;padding-left:15px">${body}</div></div>`;
  if (flow) return box(ghWaiting(esc, flow) + err);
  const choices = ghChoices(cli, accounts);
  const ghState = cli?.github?.state;
  const signIn = ghState === "signed_in" || ghState === "signed_out" ? `<button class="dbtn" ${sm} data-cred-gh>Sign in with GitHub</button>` : "";
  const paste = `<button class="dbtn${signIn ? " quiet" : ""}" ${sm} data-cred-edit="github:new">Paste a token</button>`;
  const saved = accounts.filter(a => (cli?.github_accounts ?? []).some(g => sameLogin(g.login, a.login))).map(a => `@${esc(a.login)}`);
  const help = ghState === "missing"
    ? `Make a token on ${link(esc, GITHUB_TOKEN_PAGE, "GitHub")} while signed in as the other account, then paste it`
    : signIn ? `Sign in adds the account to gh here too.${saved.length ? ` ${saved.join(", ")} ${saved.length > 1 ? "are" : "is"} in Beep already.` : ""}` : "";
  return box(`${ghChoiceRows(esc, choices, state.cliBusy)}
    <div style="display:flex;gap:6px">${signIn}${paste}</div>
    ${help ? note(help) : ""}${err}`);
}

/// The GitHub part of Git hosts: `rows` for each saved account (or, with none
/// saved, what this Mac's gh offers), `adding` for the add panel or paste form
/// when open, and `canAdd` whether "+ Add another GitHub account" shows.
function githubHtml(state: CredState, machines: ToolMachine[] | null, esc: Esc, rowState: (key: string) => RowState) {
  const e = state.edit;
  const cli = state.cli;
  const accounts = githubAccounts(state);
  const pasteForm = (id: string) => form(state, esc, { title: "GitHub", url: id === "new" ? "Another account" : "Replace the token", noteHtml: `Make one on ${link(esc, GITHUB_TOKEN_PAGE, "GitHub")} with repo access, or run ${code("gh auth login")} in Terminal and use that sign-in instead.` });
  if (!accounts.length) {
    if (e?.kind === "github") return { rows: pasteForm(e.id ?? "new"), adding: "", canAdd: false };
    const choices = ghChoices(cli, accounts);
    const offer = ghOffer(esc, cli?.github, state.cliBusy === "github", state.ghFlow);
    // Several accounts in gh here: one Use each.
    if (choices.length > 1 && !state.ghFlow) {
      offer.account = `gh on this Mac has ${choices.length} accounts`;
      offer.acts = `<button class="dbtn quiet" ${sm} data-cred-edit="github">Paste a token</button>`;
      offer.help = "";
      offer.body = `<div style="display:grid;gap:6px">${ghChoiceRows(esc, choices, state.cliBusy)}</div>`;
    }
    return { rows: hostRow(esc, { icon: GITHUB_ICON, name: "GitHub", cli: "gh", machines, editKey: "github", ...rowState("github"), offer }), adding: "", canAdd: false };
  }
  let rows = "";
  for (const acc of accounts) {
    rows += e?.kind === "github" && e.id === acc.id
      ? pasteForm(acc.id)
      : accountRow(esc, acc, { machines, cli, ...(acc.unreadable ? rowState("github") : { busy: false, error: "" }) });
  }
  if (e?.kind === "github" && (!e.id || e.id === "new")) return { rows, adding: pasteForm("new"), canAdd: false };
  if (state.ghAdd || state.ghFlow) return { rows, adding: addAccount(esc, state, accounts), canAdd: false };
  return { rows, adding: "", canAdd: true };
}

const head = (text: string, top: number) => `<div style="font-size:11px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;color:var(--dim);margin:${top}px 0 2px">${text}</div>`;
const addButton = (attr: string, text: string) => `<button class="dbtn" style="font-size:11.5px;padding:3px 9px" ${attr}>${text}</button>`;

/// The whole section (see `CredState`). `machines`: the machines list, or
/// null when it is not known.
export function credentialsHtml(state: CredState, machines: ToolMachine[] | null, esc: Esc, ago: Ago) {
  const list = state.list ?? [];
  const e = state.edit;
  const cli = state.cli;
  const rowState = (key: string): RowState => ({ busy: state.cliBusy === key, error: state.cliError?.key === key ? state.cliError.message : "" });
  const github = githubHtml(state, machines, esc, rowState);
  let html = head("Git hosts", 4) + github.rows;
  for (const c of list) {
    if (c.kind !== "forgejo") continue;
    const page = forgejoTokenPage(c.url);
    const key = `forgejo:${c.url}`;
    html += e?.kind === "forgejo" && e.id === c.id
      ? form(state, esc, { title: hostOf(c.url), url: "Replace the token", noteHtml: page ? `Make one at ${link(esc, page, hostOf(page))}, with repository and issue access.` : undefined })
      : hostRow(esc, { icon: FORGEJO_ICON, name: hostOf(c.url), cred: c, cli: "teacli", machines, editKey: `forgejo:${c.id}`, cliKey: teaHas(cli, c.url) ? key : "", check: state.checks?.[c.id], ...rowState(key) });
  }
  // Servers teacli here has a token for that Beep does not have yet.
  for (const s of teaOffers(cli, list)) {
    const key = `forgejo:${s.url}`;
    const { busy, error } = rowState(key);
    html += hostRow(esc, {
      icon: FORGEJO_ICON, name: hostOf(s.url), cli: "teacli", machines: null, editKey: "forgejo", error,
      offer: { account: "teacli on this Mac", acts: useButton(esc, key, "Use teacli sign-in", busy), help: "Signed in with teacli here, not in Beep yet" },
    });
  }
  // What is being added sits under the hosts, the add buttons side by side under that.
  html += github.adding;
  const addingForgejo = e?.kind === "forgejo" && !e.id;
  if (addingForgejo) html += form(state, esc, { title: "New Forgejo server", url: "one token per server, teacli on every machine", noteHtml: `Make one on your server at ${code("/user/settings/applications")}, with repository and issue access. Or run ${code("teacli login")} on this Mac and use that sign-in.` });
  const adds = (github.canAdd ? addButton(`data-cred-gh-add="1"`, "+ Add another GitHub account") : "")
    + (addingForgejo ? "" : addButton(`data-cred-edit="forgejo"`, "+ Add a Forgejo server"));
  if (adds) html += `<div style="display:flex;flex-wrap:wrap;gap:6px;margin-top:8px">${adds}</div>`;
  html += head("API keys", 14);
  html += `<div style="font-size:12.5px;color:var(--muted);margin:4px 0 8px;max-width:60ch">Your own keys, one per provider. Beep stores them encrypted, shows only the last 4 characters, and, when you choose, gives each to commands and terminals on your machines as its env var. Agents do not get API keys.</div>`;
  for (const p of state.providers ?? []) {
    const cred = list.find(c => c.kind === "api_key" && c.provider === p.slug);
    html += e?.kind === "api_key" && e.provider === p.slug
      ? form(state, esc, { title: p.name, env: p.env_var, url: p.env_var })
      : keyRow(esc, ago, p, cred);
  }
  return `<div style="display:grid">${html}</div>`;
}

/// The edit a `data-cred-edit` value names, with what the form starts with.
export function editFor(key: string, list: Credential[] | null | undefined): CredEdit | null {
  if (key === "github") return { kind: "github" };
  if (key.startsWith("github:")) return { kind: "github", id: key.slice("github:".length) };
  if (key === "forgejo") return { kind: "forgejo" };
  if (key.startsWith("forgejo:")) {
    const id = key.slice("forgejo:".length);
    const c = (list ?? []).find(x => x.id === id);
    return c ? { kind: "forgejo", id, url: c.kind === "forgejo" ? c.url : undefined } : { kind: "forgejo" };
  }
  if (key.startsWith("api:")) {
    const provider = key.slice(4);
    const c = (list ?? []).find(x => x.kind === "api_key" && x.provider === provider);
    return { kind: "api_key", provider, toMachines: c ? c.to_machines : true };
  }
  return null;
}
