// Settings, Agent accounts: how the coding agents sign in on your machines.

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
interface CliLogin {
  tool: string;
  signed_in: boolean;
  account?: string | null;
}

/// A machine with Antigravity and whether it is signed in there, as the
/// machine last reported it.
interface AgyMachine {
  id: string;
  name: string;
  online: boolean;
  signed_in: boolean;
}

/// The coding agents that keep accounts.
export type AccountAgent = "claude" | "codex";

/// A named account of a coding agent, such as Claude Code's Work (NamedView).
export interface NamedAccount {
  agent: AccountAgent;
  name: string;
  token: AgentToken;
}

/// What `agent_accounts` answers (AccountsView).
export interface AgentAccounts {
  claude: AgentToken | null;
  codex_key: AgentToken | null;
  machines: AgentMachine[];
  named: NamedAccount[];
  /// Each machine's own Claude Code sign-in by machine id, as it last
  /// reported it. Added here from `list_machines`.
  claude_logins?: Map<string, CliLogin>;
  /// The machines with Antigravity. Added here from `list_machines`.
  agy_machines?: AgyMachine[];
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

/// The named account being added.
interface AccountDraft {
  agent: AccountAgent;
  error: string;
}

export interface AgentAccountsState {
  data: AgentAccounts | null | undefined;
  add?: AccountDraft | null;
  /// Why removing one of the agent's named accounts failed.
  namedError?: { agent: AccountAgent; text: string } | null;
  edit?: string | null;
  busy?: string;
  error?: string;
  claudeFlow?: boolean;
  claudeError?: string;
  codex?: Record<string, CodexFlow>;
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

/// The longest account name taken, as the desktop checks it.
const MAX_ACCOUNT_NAME = 32;

/// The name a new account is saved under, or an error to show.
export function accountName(name: string): { name: string; error: null } | { name: null; error: string } {
  const trimmed = name.trim();
  if (!trimmed) return { name: null, error: "Name the account first, such as Work." };
  if ([...trimmed].length > MAX_ACCOUNT_NAME) return { name: null, error: `An account name has at most ${MAX_ACCOUNT_NAME} characters.` };
  if (/[~/\p{Cc}]/u.test(trimmed)) return { name: null, error: "An account name cannot hold ~, / or control characters." };
  if (trimmed.toLowerCase() === "default") return { name: null, error: "Default is the account without a name; pick another name." };
  return { name: trimmed, error: null };
}

/// What a `processes` update says of the node task `task`: whether it still runs. Null when the
/// update carries no task list; false when `task` is not in it or ended.
export function codexTaskRunning(data: unknown, task: number): boolean | null {
  const list = (data as { Process?: { Tasks?: { tasks?: unknown } } } | null)?.Process?.Tasks?.tasks;
  if (!Array.isArray(list)) return null;

  const found = list.find((t): t is { id: unknown; state: unknown } => typeof t === "object" && t !== null && "id" in t && t.id === task);
  return found?.state === "running";
}
