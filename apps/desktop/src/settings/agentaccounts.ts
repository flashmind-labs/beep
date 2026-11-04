// Settings, Agent accounts: how the coding agents sign in on your machines.

import { harnessTile } from "../agents/harnessicon.ts";

type Esc = (s: unknown) => string;
/// "today", "3 days ago": how long ago a time in milliseconds was.
type Ago = (ms: number) => string;

/// A saved token or key (TokenView).
interface AgentToken {
  last4: string;
  /// Milliseconds since the Unix epoch.
  created_at: number;
  unreadable: boolean;
}

/// One machine with Claude Code or Codex (agent_accounts' MachineView).
interface AgentMachine {
  id: string;
  name: string;
  online: boolean;
  claude: boolean;
  codex: boolean;
  claude_sessions: number;
  /// Null until the machine said.
  codex_signed_in: boolean | null;
}

/// A CLI sign-in a machine reports, such as Claude Code's (LoginView).
export interface CliLogin {
  tool: string;
  signed_in: boolean;
  account?: string | null;
}

/// What `agent_accounts` answers (AccountsView).
export interface AgentAccounts {
  claude: AgentToken | null;
  codex_key: AgentToken | null;
  machines: AgentMachine[];
  /// Each machine's own Claude Code sign-in by machine id, as it last
  /// reported it. Added here from `list_machines`.
  claude_logins?: Map<string, CliLogin>;
}

/// A Codex sign-in under way on a machine (CodexLoginView).
export interface CodexLogin {
  task: number;
  url: string;
  code: string;
  /// Milliseconds since the Unix epoch.
  expires_at: number;
}

/// A machine's Codex sign-in: starting, waiting on its code, or failed.
export type CodexFlow =
  | { starting: true; task?: undefined; code?: undefined; error?: undefined }
  | (CodexLogin & { copied?: boolean; starting?: undefined; error?: undefined })
  | { error: string; starting?: undefined; task?: undefined; code?: undefined };

export interface AgentAccountsState {
  data: AgentAccounts | null | false | undefined;
  edit?: string | null;
  busy?: string;
  error?: string;
  claudeFlow?: boolean;
  claudeError?: string;
  codex?: Record<string, CodexFlow>;
}

const box = (inner: string) => `<div class="dbox">${inner}</div>`;
const row = (name: string, off: boolean, act: string) => `<div class="dline dsm dpy"><span class="ddot${off ? " off" : ""}"></span><span>${name}</span><span class="dacts dmut">${act}</span></div>`;
/// A muted line in place of a list with nothing in it.
const none = (text: string) => `<div class="dfine dpy">${text}</div>`;

/// "2 sessions now", "idle", or "offline · gets it when back": how a
/// machine with Claude Code stands with the token.
function usedOn(machine: AgentMachine) {
  if (!machine.online) return "offline · gets it when back";
  const n = machine.claude_sessions || 0;
  if (!n) return "idle";
  return n === 1 ? "1 session now" : `${n} sessions now`;
}

/// How a machine's own Claude Code sign-in stands: "signed in as you@example.com",
/// "not signed in", or "not checked" when it never said.
function ownLogin(login: CliLogin | null | undefined) {
  if (!login) return "not checked";
  if (!login.signed_in) return "not signed in";
  return login.account ? `signed in as ${login.account}` : "signed in";
}

/// "14:32 left" until `expiresAt` (ms), or "expired".
function timeLeft(expiresAt: number, now: number) {
  const s = Math.floor((expiresAt - now) / 1000);
  if (!(s > 0)) return "expired";
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")} left`;
}

type TokenDraft = { error: string; agent?: undefined; secret?: undefined } | { agent: "claude" | "codex"; secret: string; error?: undefined };

/// What Save sends for the token being typed, or an error to show.
export function tokenDraft(agent: string | null | undefined, secret: string | undefined): TokenDraft {
  secret = String(secret || "").trim();
  if (agent !== "claude" && agent !== "codex") return { error: "Nothing to save." };
  if (!secret) return { error: agent === "claude" ? "Paste the token first." : "Paste the key first." };
  if (/\s/.test(secret)) return { error: "That does not look like a token: it has spaces in it." };
  if (agent === "claude" && secret.startsWith("sk-ant-api")) return { error: "That is an Anthropic API key. Paste the token claude setup-token prints." };
  return { agent, secret };
}

/// The paste form for `agent`, when it is the one being edited.
function form(state: AgentAccountsState, esc: Esc, agent: "claude" | "codex") {
  if (state.edit !== agent) return "";
  const claude = agent === "claude";
  const note = claude
    ? `Run <code>claude setup-token</code> in Terminal and paste the token it prints.`
    : `An OpenAI API key. Codex uses it on every machine instead of signing in.`;
  return `<div class="dgrid8 dmt6">
    <div class="dline"><input id="agSecret" type="password" autocomplete="off" spellcheck="false" placeholder="${claude ? "sk-ant-oat01-…" : "sk-…"}" aria-label="${claude ? "Claude Code token" : "Codex API key"}" class="dsecret">
    <button class="dbtn pri" id="agSave" data-ag-agent="${agent}"${state.busy ? " disabled" : ""}>${state.busy === agent ? "Saving…" : "Save"}</button><button class="dbtn quiet" id="agCancel">Cancel</button></div>
    <span class="dsub">${note}</span>
    <span class="derr" id="agErr">${esc(state.error || "")}</span></div>`;
}

function claudeHtml(state: AgentAccountsState, data: AgentAccounts, esc: Esc, ago: Ago) {
  const token = data.claude;
  const on = (data.machines ?? []).filter(m => m.claude);
  const head = (sub: string) => `<div class="dline">${harnessTile("claude")}<span class="dstack"><b class="dtitle">Claude Code</b>${sub ? `<span class="dsub">${sub}</span>` : ""}</span>`;
  const err = state.claudeError ? `<div class="derr dmt6">${esc(state.claudeError)}</div>` : "";
  if (state.claudeFlow) {
    return box(`${head("")}<span class="dpush"><button class="dbtn quiet small" data-ag-claude-cancel>Cancel</button></span></div>
      <div class="dfine dline dmt6"><span class="dspin"></span>Approve in your browser. Beep saves the token once you do.</div>`);
  }
  const edit = form(state, esc, "claude");
  if (!token || token.unreadable) {
    const why = token?.unreadable ? "Beep can no longer read the saved token. Sign in again." : "Sign in for all to give every machine one token. Beep keeps it encrypted and never writes it to their disks.";
    const rows = on.map(m => row(esc(m.name), !m.online, esc(ownLogin(data.claude_logins?.get(m.id))))).join("");
    return box(`${head(on.length ? "signs in on each machine" : "")}<span class="dacts"><button class="dbtn${on.length ? "" : " pri"}" data-ag-claude-sign>${on.length ? "Sign in for all" : "Sign in"}</button>${edit ? "" : `<button class="dbtn quiet" data-ag-edit="claude">Paste a token</button>`}</span></div>
      ${rows ? `<div class="dmt6">${rows}</div>` : ""}<div class="dfine dmt6">${why}</div>${edit}${err}`);
  }
  const sub = `signed in ${esc(ago(token.created_at))} · <code>…${esc(token.last4)}</code>`;
  const used = on.length
    ? on.map(m => row(esc(m.name), !m.online, esc(usedOn(m)))).join("")
    : none("No machine has Claude Code yet.");
  return box(`${head(sub)}<span class="dacts"><button class="dbtn" data-ag-claude-sign>Replace</button><button class="dbtn warn" data-ag-revoke="claude"${state.busy === "revoke:claude" ? " disabled" : ""}>Revoke</button></span></div>
    ${edit}${err}<div class="dfine dmt6">Used on</div>${used}
    ${edit ? "" : `<button class="dbtn quiet small" data-ag-edit="claude">Paste a token instead</button>`}`);
}

/// One machine's Codex row: its sign-in, the code it waits on, or why
/// it failed.
function codexRow(state: AgentAccountsState, esc: Esc, m: AgentMachine, now: number) {
  const flow = state.codex?.[m.id];
  if (flow?.code) {
    return `<div class="dflow">
      <span>${esc(m.name)}: open <b>${esc(flow.url.replace(/^https:\/\//, ""))}</b> and enter</span>
      <span class="dflowcode">${esc(flow.code)}</span>
      <span class="dmuted"><span class="dspin"></span>Waiting for you to approve · ${esc(timeLeft(flow.expires_at, now))}<span class="dacts"><button class="dbtn small" data-ag-copy="${esc(flow.code)}">${flow.copied ? "Copied" : "Copy code"}</button><button class="dbtn quiet small" data-ag-open="${esc(flow.url)}">Open page</button><button class="dbtn quiet small" data-ag-codex-cancel="${esc(m.id)}">Cancel</button></span></span></div>`;
  }
  const err = flow?.error ? `<div class="derr dind">${esc(flow.error)}</div>` : "";
  if (!m.online) return row(esc(m.name), true, "offline") + err;
  if (flow?.starting) return row(esc(m.name), false, `<span class="dspin"></span>starting sign-in`);
  if (m.codex_signed_in) return row(esc(m.name), false, "signed in") + err;
  const label = m.codex_signed_in === false ? "" : `<span>not checked</span>`;
  return row(esc(m.name), false, `${label}<button class="dbtn small" data-ag-codex-sign="${esc(m.id)}">Sign in</button>`) + err;
}

function codexHtml(state: AgentAccountsState, data: AgentAccounts, esc: Esc, ago: Ago, now: number) {
  const key = data.codex_key;
  const on = (data.machines ?? []).filter(m => m.codex);
  const sub = key && !key.unreadable
    ? `API key <code>…${esc(key.last4)}</code> on every machine · added ${esc(ago(key.created_at))}`
    : key ? "Beep can no longer read the saved API key" : "signs in on each machine";
  const acts = key
    ? `<button class="dbtn small" data-ag-edit="codex">Replace key</button><button class="dbtn quiet warn small" data-ag-revoke="codex"${state.busy === "revoke:codex" ? " disabled" : ""}>Remove key</button>`
    : `<button class="dbtn" data-ag-edit="codex">Use an API key</button>`;
  const edit = form(state, esc, "codex");
  const rows = on.length
    ? on.map(m => codexRow(state, esc, m, now)).join("")
    : none("No machine has Codex yet.");
  return box(`<div class="dline">${harnessTile("codex")}<span class="dstack"><b class="dtitle">Codex</b><span class="dsub">${sub}</span></span><span class="dacts">${edit ? "" : acts}</span></div>
    ${edit}<div class="dmt6">${rows}</div>`);
}

/// The whole section (see `AgentAccountsState`). Drawn once `data` came.
export function agentAccountsHtml(state: AgentAccountsState, esc: Esc, ago: Ago, now = Date.now()) {
  const data = state.data;
  if (data === null) return `<div class="dmuted">Could not load your agent accounts.</div>`;
  if (data === false) return `<div class="dmuted">The server needs an update before it keeps agent accounts.</div>`;
  // Never called before the answer came.
  return `<div class="dlead">How your coding agents sign in on your machines.</div>${claudeHtml(state, data!, esc, ago)}${codexHtml(state, data!, esc, ago, now)}`;
}
