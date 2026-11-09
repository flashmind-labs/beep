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
