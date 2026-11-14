// Settings, Credentials: git hosts (GitHub, one row per Forgejo server) and
// API keys, one per provider, that every machine gets as an env var.

/// A saved credential (CredentialView), with what its kind adds.
export type Credential = {
  id: string;
  last4: string;
  account: string | null;
  env_var: string;
  /// Milliseconds since the Unix epoch.
  created_at: number;
  /// Beep can no longer open it: the row asks for it again.
  unreadable?: boolean;
} & ({ kind: "github" } | { kind: "forgejo"; url: string } | { kind: "api_key"; provider: string });

type Forgejo = Extract<Credential, { kind: "forgejo" }>;
export type ApiKey = Extract<Credential, { kind: "api_key" }>;

/// A provider an API key can be saved for (ApiProviderView).
export interface ApiProvider {
  provider: string;
  name: string;
  env_var: string;
}

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

/// A CLI's sign-in on this Mac (LoginView).
type CliLogin = { state: "signed_in"; account?: string | null } | { state: "signed_out" } | { state: "missing" };

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
type CredEdit =
  | { kind: "github"; id?: string }
  | { kind: "forgejo"; id?: string; url?: string }
  /// `provider` is null while a new key's provider is still to pick.
  | { kind: "api_key"; id?: string; provider: string | null };

/// What `save_credential` takes (NewCredential).
type NewCredential =
  | { kind: "github"; secret: string }
  | { kind: "forgejo"; url: string; secret: string }
  | { kind: "api_key"; provider: string; secret: string };

/// The section's state.
export interface CredState {
  list?: Credential[] | null;
  providers?: ApiProvider[];
  github?: GitHubAccount[];
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

/// `git.example.com` from `https://git.example.com/`.
export function hostOf(url: string | null | undefined) {
  return String(url || "").replace(/^[a-z]+:\/\//i, "").replace(/\/+$/, "");
}

type Draft = { error: string; credential?: undefined } | { credential: NewCredential; error?: undefined };

/// What Save sends for the row being edited, or an error to show. `field` is
/// a new Forgejo server's URL or a new API key's provider.
export function draft(edit: CredEdit | null | undefined, field: string | undefined, secret: string | undefined): Draft {
  secret = String(secret || "").trim();
  if (!edit) return { error: "Nothing to save." };
  if (!secret) return { error: edit.kind === "api_key" ? "Paste the key first." : "Paste the token first." };

  if (edit.kind === "api_key") {
    const provider = edit.provider ?? String(field || "");
    if (!provider) return { error: "Pick a provider." };
    return { credential: { kind: "api_key", provider, secret } };
  }

  if (edit.kind === "forgejo") {
    const u = String(field || edit.url || "").trim();
    if (!/^https?:\/\/[^\s/]+/i.test(u)) return { error: "The server URL starts with https://, such as https://git.example.com." };
    return { credential: { kind: "forgejo", url: u, secret } };
  }

  return { credential: { kind: "github", secret } };
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

/// A Forgejo or Gitea server's page for new access tokens, at the server's
/// root whatever path the saved URL has (a repository's, say), or null for
/// a URL that is not one.
export function forgejoTokenPage(url: string) {
  try {
    const u = new URL(String(url || "").trim());
    if (!/^https?:$/.test(u.protocol)) return null;
    return `${u.origin}/user/settings/applications`;
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


/// The GitHub accounts: from `list_github_accounts` when it was read, else
/// the one GitHub row of the credentials list, as the default.
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

/// The edit a `data-cred-edit` value names, with what the form starts with.
export function editFor(key: string, list: Credential[] | null | undefined): CredEdit | null {
  if (key === "github") return { kind: "github" };
  if (key.startsWith("github:")) return { kind: "github", id: key.slice("github:".length) };
  if (key === "forgejo") return { kind: "forgejo" };
  if (key === "api_key") return { kind: "api_key", provider: null };

  if (key.startsWith("api_key:")) {
    const id = key.slice("api_key:".length);
    const c = (list ?? []).find(x => x.id === id);
    return c?.kind === "api_key" ? { kind: "api_key", id, provider: c.provider } : null;
  }

  if (key.startsWith("forgejo:")) {
    const id = key.slice("forgejo:".length);
    const c = (list ?? []).find(x => x.id === id);
    return c ? { kind: "forgejo", id, url: c.kind === "forgejo" ? c.url : undefined } : { kind: "forgejo" };
  }

  return null;
}

/// The saved API keys, in the providers' order.
export function apiKeys(state: CredState): ApiKey[] {
  const order = (state.providers ?? []).map(p => p.provider);
  return (state.list ?? []).filter((c): c is ApiKey => c.kind === "api_key").sort((a, b) => order.indexOf(a.provider) - order.indexOf(b.provider));
}

/// The providers with no key saved yet.
export function unsavedProviders(state: CredState): ApiProvider[] {
  const saved = apiKeys(state);
  return (state.providers ?? []).filter(p => !saved.some(k => k.provider === p.provider));
}
