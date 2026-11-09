// Settings, Credentials: git hosts (GitHub, one row per Forgejo server). API
// keys are kept on the server only, never here.

/// A saved credential (CredentialView), with what its kind adds.
export type Credential = {
  id: string;
  last4: string;
  account: string | null;
  env_var: string;
  /// Milliseconds since the Unix epoch.
  created_at: number;
  /// The server can no longer open it: the row asks for it again.
  unreadable?: boolean;
} & ({ kind: "github" } | { kind: "forgejo"; url: string });

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
type CredEdit =
  | { kind: "github"; id?: string }
  | { kind: "forgejo"; id?: string; url?: string };

/// What `save_credential` takes (NewCredential).
type NewCredential =
  | { kind: "github"; secret: string }
  | { kind: "forgejo"; url: string; secret: string };

/// The section's state.
export interface CredState {
  list?: Credential[] | null;
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

/// What Save sends for the row being edited, or an error to show.
export function draft(edit: CredEdit | null | undefined, url: string | undefined, secret: string | undefined): Draft {
  secret = String(secret || "").trim();
  if (!edit) return { error: "Nothing to save." };
  if (!secret) return { error: "Paste the token first." };

  if (edit.kind === "forgejo") {
    const u = String(url || edit.url || "").trim();
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

export const GITHUB_ICON = `<svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor" aria-hidden="true"><path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"/></svg>`;
export const FORGEJO_ICON = `<svg viewBox="0 0 212 212" width="15" height="15" aria-hidden="true"><g fill="none" transform="translate(6 6)"><path d="M58 168V70a50 50 0 0 1 50-50h20" stroke="#f60" stroke-width="25"/><path d="M58 168v-30a50 50 0 0 1 50-50h20" stroke="#d40000" stroke-width="25"/><circle cx="142" cy="20" r="18" stroke="#f60" stroke-width="15"/><circle cx="142" cy="88" r="18" stroke="#d40000" stroke-width="15"/><circle cx="58" cy="180" r="18" stroke="#d40000" stroke-width="15"/></g></svg>`;
export const EDIT_ICON = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17 3a2.85 2.85 0 0 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>`;
export const REMOVE_ICON = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/></svg>`;

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

  return null;
}
