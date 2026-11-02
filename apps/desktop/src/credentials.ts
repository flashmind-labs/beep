// Settings, Credentials: git hosts (GitHub, one row per Forgejo server)
// then one API key per provider. Secrets are typed here, sent once and
// never shown again: the server gives back only their last 4 characters.

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

/// The section's state. `github` is the GitHub accounts (undefined when the
/// server could not list them) and `ghAdd` whether "Add another GitHub
/// account" is open. `cli` is what gh and teacli on this Mac are signed in
/// to (undefined until checked, null when the check failed); `cliBusy` the
/// `data-cred-cli` key being saved and `cliError` its refusal; `ghFlow`
/// gh's browser sign-in under way.
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

/// "gh on build-box, mac-box", or "" when the machines are not known.
export function toolLine(cli: string, machines: ToolMachine[] | null) {
  const on = machinesWith(machines, cli);
  if (on === null) return "";
  return on.length ? `${cli} on ${on.join(", ")}` : `${cli} on no machine yet`;
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
const link = (esc: Esc, href: string, text: string) => `<a href="${esc(href)}" data-cred-open="${esc(href)}" style="color:var(--emerald)">${esc(text)}</a>`;
const code = (text: string) => `<code style="font-size:11px">${text}</code>`;
/// A muted line under a row's name.
const note = (html: string) => `<span style="font-size:11.5px;color:var(--muted);padding-left:15px">${html}</span>`;
/// Why a sign-in was refused, under its row.
const rowError = (esc: Esc, error: string | undefined) => error ? `<span class="derr" style="padding-left:15px">${esc(error)}</span>` : "";

/// "Use gh sign-in", or "Checking…" while it is being saved.
const useButton = (esc: Esc, key: string, label: string, busy: boolean) =>
  `<button class="dbtn pri" ${sm} data-cred-cli="${esc(key)}"${busy ? " disabled" : ""}>${busy ? "Checking…" : esc(label)}</button>`;

/// The code gh's browser sign-in waits for, large and selectable, or a
/// spinner until gh prints it.
function ghWaiting(esc: Esc, flow: GhFlow) {
  const entry = flow.code
    ? `<span>Enter code <code style="font-family:var(--mono);font-size:18px;font-weight:600;letter-spacing:.1em;user-select:all;-webkit-user-select:all;cursor:text">${esc(flow.code)}</code> on ${link(esc, flow.url, "GitHub")}${flow.copied ? `<span style="color:var(--muted)"> (copied)</span>` : ""}</span>`
    : "";
  return `<div style="margin-left:15px;padding:8px 10px;border-radius:8px;border:1px solid var(--border);font-size:12.5px;display:grid;gap:6px">${entry}
    <span style="display:flex;gap:6px;align-items:center;color:var(--muted)"><span class="dspin"></span>${flow.code ? "Waiting for GitHub" : "Starting gh…"}</span></div>`;
}

/// What a git host row offers when nothing usable is saved: the account
/// line, the buttons, help under it and a body under that.
export interface Offer {
  account?: string;
  acts: string;
  help: string;
  body?: string;
}

/// What the GitHub row offers when no usable token is saved, from this
/// Mac's gh: its sign-in, signing it in, or a link to make a token.
/// `gh` is undefined until checked and null when the check failed; `flow`
/// is gh's browser sign-in under way, if any.
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

/// One git host row. `offer` takes the place of Add when nothing usable is
/// saved; `cliKey` adds "Use <cli> sign-in" to a saved row the server can
/// no longer open.
interface HostRow extends Partial<RowState> {
  name: string;
  cred?: Credential;
  cli: string;
  machines: ToolMachine[] | null;
  editKey: string;
  offer?: Offer;
  cliKey?: string;
}

/// One git host: name, login, last 4 and where its CLI is.
function hostRow(esc: Esc, { name, cred, cli, machines, editKey, offer, cliKey, busy, error }: HostRow) {
  const usable = cred && !cred.unreadable;
  const line = [cred ? shown(esc, cred, "token") : "", esc(toolLine(cli, machines))].filter(Boolean).join(" · ");
  const account = usable
    ? cred.account ? `@${cred.account}` : "not checked: the server could not reach it"
    : offer?.account ?? "";
  const use = cliKey && !usable ? useButton(esc, cliKey, `Use ${cli} sign-in`, !!busy) : "";
  const acts = cred
    ? `${use}<button class="dbtn" ${sm} data-cred-edit="${esc(editKey)}">Replace</button><button class="dbtn quiet" ${sm} data-cred-remove="${esc(cred.id)}" aria-label="Remove ${esc(name)}">Remove</button>`
    : offer?.acts ?? `<button class="dbtn" ${sm} data-cred-edit="${esc(editKey)}">Add token</button>`;
  const help = !usable && offer?.help ? note(offer.help) : "";
  const body = !usable && offer?.body ? offer.body : "";
  return `<div style="padding:9px 0;border-bottom:1px solid var(--border);display:grid;gap:3px;font-size:13px">
    <div style="display:flex;align-items:center;gap:8px;min-width:0"><span class="ddot${cred ? "" : " off"}"></span><b style="font-weight:600">${esc(name)}</b>${account ? `<span style="color:var(--muted);font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${esc(account)}</span>` : ""}<span style="margin-left:auto;display:flex;gap:6px;flex:none">${acts}</span></div>
    ${line ? note(line) : ""}${help}${body}${rowError(esc, error)}</div>`;
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
  return choices.map(g => `<div style="display:flex;align-items:center;gap:8px;padding-left:15px;font-size:12.5px"><span style="color:var(--muted)">gh on this Mac as @${esc(g.login)}</span>${g.active ? `<span style="font-size:10.5px;color:var(--dim)">active</span>` : ""}<span style="margin-left:auto">${ghUse(esc, g.login, cliBusy === `gh:${g.login}`)}</span></div>`).join("");
}

/// What an account row needs besides the account.
interface AccountRowState extends RowState {
  machines: ToolMachine[] | null;
  cli: CliLogins | null | undefined;
}

/// One saved GitHub account: login, default tag, orgs, last 4, and Make
/// default, Replace and Remove.
function accountRow(esc: Esc, acc: GitHubAccount, { machines, cli, busy, error }: AccountRowState) {
  const usable = !acc.unreadable;
  const orgs = acc.orgs?.length ? `orgs ${acc.orgs.map(esc).join(", ")}` : "";
  const role = acc.default ? "GH_TOKEN on your machines" : "projects of its login and orgs";
  const line = [shown(esc, acc, "token"), orgs, role, esc(toolLine("gh", machines))].filter(Boolean).join(" · ");
  const ghHere = (cli?.github_accounts ?? []).some(g => sameLogin(g.login, acc.login)) || cli?.github?.state === "signed_in";
  const use = !usable && ghHere ? useButton(esc, "github", "Use gh sign-in", busy) : "";
  const make = acc.default ? "" : `<button class="dbtn" ${sm} data-cred-gh-default="${esc(acc.id)}">Make default</button>`;
  const tag = acc.default ? `<span style="font-size:10.5px;padding:1px 6px;border-radius:999px;border:1px solid var(--border);color:var(--muted);flex:none">default</span>` : "";
  return `<div style="padding:9px 0;border-bottom:1px solid var(--border);display:grid;gap:3px;font-size:13px">
    <div style="display:flex;align-items:center;gap:8px;min-width:0"><span class="ddot${usable ? "" : " off"}"></span><b style="font-weight:600">GitHub</b>${acc.login ? `<span style="color:var(--muted);font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">@${esc(acc.login)}</span>` : ""}${tag}<span style="margin-left:auto;display:flex;gap:6px;flex:none">${use}${make}<button class="dbtn" ${sm} data-cred-edit="github:${esc(acc.id)}">Replace</button><button class="dbtn quiet" ${sm} data-cred-remove="${esc(acc.id)}" aria-label="Remove @${esc(acc.login)}">Remove</button></span></div>
    ${note(line)}${rowError(esc, error)}</div>`;
}

/// Adding another GitHub account: gh's accounts here, gh's browser
/// sign-in, which adds one to gh too, or a pasted token.
function addAccount(esc: Esc, state: CredState, accounts: GitHubAccount[]) {
  const cli = state.cli;
  const flow = state.ghFlow;
  const err = state.cliError?.key === "github" ? `<span class="derr" style="padding-left:15px">${esc(state.cliError.message)}</span>` : "";
  const top = `<div style="display:flex;align-items:center;gap:8px"><b style="font-weight:600">Add another GitHub account</b><span style="margin-left:auto">${flow ? `<button class="dbtn quiet" ${sm} data-cred-gh-cancel>Cancel</button>` : `<button class="dbtn quiet" ${sm} data-cred-gh-add="">Cancel</button>`}</span></div>`;
  if (flow) return `<div style="padding:10px 0;border-bottom:1px solid var(--border);display:grid;gap:6px;font-size:13px">${top}${ghWaiting(esc, flow)}${err}</div>`;
  const choices = ghChoices(cli, accounts);
  const ghState = cli?.github?.state;
  const signIn = ghState === "signed_in" || ghState === "signed_out" ? `<button class="dbtn" ${sm} data-cred-gh>Sign in with GitHub</button>` : "";
  const paste = `<button class="dbtn${signIn ? " quiet" : ""}" ${sm} data-cred-edit="github:new">Paste a token</button>`;
  const saved = accounts.filter(a => (cli?.github_accounts ?? []).some(g => sameLogin(g.login, a.login))).map(a => `@${esc(a.login)}`);
  const help = ghState === "missing"
    ? `Make a token on ${link(esc, GITHUB_TOKEN_PAGE, "GitHub")} while signed in as the other account, then paste it`
    : signIn ? `Sign in adds the account to gh here too.${saved.length ? ` ${saved.join(", ")} ${saved.length > 1 ? "are" : "is"} in Beep already.` : ""}` : "";
  return `<div style="padding:10px 0;border-bottom:1px solid var(--border);display:grid;gap:6px;font-size:13px">${top}
    ${ghChoiceRows(esc, choices, state.cliBusy)}
    <div style="display:flex;gap:6px;padding-left:15px">${signIn}${paste}</div>
    ${help ? note(help) : ""}${err}</div>`;
}

/// The GitHub part of Git hosts: each saved account, then a way to add
/// another; with none saved, what this Mac's gh offers.
function githubHtml(state: CredState, machines: ToolMachine[] | null, esc: Esc, rowState: (key: string) => RowState) {
  const e = state.edit;
  const cli = state.cli;
  const accounts = githubAccounts(state);
  const pasteForm = (id: string) => form(state, esc, { title: "GitHub", url: id === "new" ? "Another account" : "Replace the token", noteHtml: `Make one on ${link(esc, GITHUB_TOKEN_PAGE, "GitHub")} with repo access, or run ${code("gh auth login")} in Terminal and use that sign-in instead.` });
  if (!accounts.length) {
    if (e?.kind === "github") return pasteForm(e.id ?? "new");
    const choices = ghChoices(cli, accounts);
    const offer = ghOffer(esc, cli?.github, state.cliBusy === "github", state.ghFlow);
    // Several accounts in gh here: one Use each.
    if (choices.length > 1 && !state.ghFlow) {
      offer.account = `gh on this Mac has ${choices.length} accounts`;
      offer.acts = `<button class="dbtn quiet" ${sm} data-cred-edit="github">Paste a token</button>`;
      offer.help = "";
      offer.body = ghChoiceRows(esc, choices, state.cliBusy);
    }
    return hostRow(esc, { name: "GitHub", cli: "gh", machines, editKey: "github", ...rowState("github"), offer });
  }
  let html = "";
  for (const acc of accounts) {
    html += e?.kind === "github" && e.id === acc.id
      ? pasteForm(acc.id)
      : accountRow(esc, acc, { machines, cli, ...(acc.unreadable ? rowState("github") : { busy: false, error: "" }) });
  }
  if (e?.kind === "github" && (!e.id || e.id === "new")) html += pasteForm("new");
  else if (state.ghAdd || state.ghFlow) html += addAccount(esc, state, accounts);
  else html += `<button class="dbtn" style="font-size:11.5px;padding:3px 9px;margin-top:8px;justify-self:start" data-cred-gh-add="1">+ Add another GitHub account</button>`;
  return html;
}

const head = (text: string, top: number) => `<div style="font-size:11px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;color:var(--dim);margin:${top}px 0 2px">${text}</div>`;

/// The whole section (see `CredState`). `machines`: the machines list, or
/// null when it is not known.
export function credentialsHtml(state: CredState, machines: ToolMachine[] | null, esc: Esc, ago: Ago) {
  const list = state.list ?? [];
  const e = state.edit;
  const cli = state.cli;
  const rowState = (key: string): RowState => ({ busy: state.cliBusy === key, error: state.cliError?.key === key ? state.cliError.message : "" });
  let html = head("Git hosts", 4);
  html += githubHtml(state, machines, esc, rowState);
  for (const c of list) {
    if (c.kind !== "forgejo") continue;
    const page = forgejoTokenPage(c.url);
    const key = `forgejo:${c.url}`;
    html += e?.kind === "forgejo" && e.id === c.id
      ? form(state, esc, { title: hostOf(c.url), url: "Replace the token", noteHtml: page ? `Make one at ${link(esc, page, hostOf(page))}, with repository and issue access.` : undefined })
      : hostRow(esc, { name: hostOf(c.url), cred: c, cli: "teacli", machines, editKey: `forgejo:${c.id}`, cliKey: teaHas(cli, c.url) ? key : "", ...rowState(key) });
  }
  // Servers teacli here has a token for that Beep does not have yet.
  for (const s of teaOffers(cli, list)) {
    const key = `forgejo:${s.url}`;
    const { busy, error } = rowState(key);
    html += hostRow(esc, {
      name: hostOf(s.url), cli: "teacli", machines: null, editKey: "forgejo", error,
      offer: { account: "teacli on this Mac", acts: useButton(esc, key, "Use teacli sign-in", busy), help: "Signed in with teacli here, not in Beep yet" },
    });
  }
  html += e?.kind === "forgejo" && !e.id
    ? form(state, esc, { title: "New Forgejo server", url: "one token per server, teacli on every machine", noteHtml: `Make one on your server at ${code("/user/settings/applications")}, with repository and issue access. Or run ${code("teacli login")} on this Mac and use that sign-in.` })
    : `<button class="dbtn" style="font-size:11.5px;padding:3px 9px;margin-top:8px;justify-self:start" data-cred-edit="forgejo">+ Add a Forgejo server</button>`;
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
