// The Assistant page's keys: which providers it lists and what each row shows. Pure; page.ts
// draws it.
import type { Credential } from "../settings/credentials.ts";

/// A provider the assistant can use a key for, as `ApiProvider` names it.
export interface KeyProvider {
  slug: string;
  name: string;
  /// What the key is for, in a few words.
  use: string;
  /// Where to get one.
  url: string;
}

/// The keys the assistant takes, both optional.
export const KEY_PROVIDERS: readonly KeyProvider[] = [
  { slug: "openrouter", name: "OpenRouter", use: "Chat", url: "https://openrouter.ai/keys" },
  { slug: "firecrawl", name: "Firecrawl", use: "Web search", url: "https://www.firecrawl.dev/app/api-keys" },
];

/// The one line under the keys.
export const KEYS_NOTE = "You pay these services. The keys are stored in your mesh and available to commands on your machines.";

/// A saved key, as far as the page shows it.
export interface SavedKey {
  id: string;
  last4: string;
  /// The mesh holds it but this Mac cannot open it: enter it again.
  unreadable: boolean;
}

export interface KeyRow {
  provider: KeyProvider;
  saved: SavedKey | null;
}

/// One row per provider, with its saved key when the credentials list holds one.
export function keyRows(list: readonly Credential[] | null | undefined): KeyRow[] {
  return KEY_PROVIDERS.map(provider => {
    const found = (list ?? []).find(c => c.kind === "api_key" && c.provider === provider.slug);
    return { provider, saved: found ? { id: found.id, last4: found.last4, unreadable: !!found.unreadable } : null };
  });
}

/// What `save_credential` takes for `provider`'s key. It goes to the machines, as every key does here.
export function newKey(provider: KeyProvider, secret: string) {
  return { kind: "api_key", provider: provider.slug, secret: secret.trim(), to_machines: true };
}

/// The line under the MCP servers heading.
export const MCP_NOTE = "Tools for chats on this Mac. One that changes something asks first. Results are private: web search turns off in that chat.";

/// How a listed server is doing (`McpStatus`).
export type McpState = { state: "idle" } | { state: "ready"; tools: number } | { state: "failed"; error: string };

/// A listed server, as Rust sends it: never a secret, only that one is set.
export interface McpRow {
  name: string;
  /// The command line, or the address without its credentials.
  detail: string;
  /// The names of the environment variables that are set.
  env: string[];
  token: boolean;
  status: McpState;
}

/// What a row says of its server's state, and whether that is an error.
export function mcpStatus(row: McpRow): { text: string; failed: boolean } {
  const s = row.status;
  if (s.state === "failed") return { text: "Failed", failed: true };
  if (s.state === "ready") return { text: s.tools === 1 ? "1 tool" : `${s.tools} tools`, failed: false };
  return { text: "Not started", failed: false };
}

/// The second line of a row: what it runs, and that secrets are set.
export function mcpDetail(row: McpRow) {
  const set = [...row.env, ...(row.token ? ["token"] : [])];
  return set.length ? `${row.detail} · ${set.join(", ")} set` : row.detail;
}

/// The add form: a command run on this Mac, or an address.
export interface McpForm {
  kind: "command" | "url";
  name: string;
  command: string;
  /// Lines of `NAME=value`.
  env: string;
  url: string;
  token: string;
}

export const blankMcpForm = (): McpForm => ({ kind: "command", name: "", command: "", env: "", url: "", token: "" });

/// Whether the form has what its kind needs.
export function mcpFormReady(f: McpForm) {
  return !!f.name.trim() && !!(f.kind === "command" ? f.command : f.url).trim();
}

/// What `add_mcp_server` takes for `f`. Only the fields of its kind go.
export function newMcpServer(f: McpForm) {
  const name = f.name.trim();
  return f.kind === "command"
    ? { kind: "command", name, command: f.command.trim(), env: f.env }
    : { kind: "url", name, url: f.url.trim(), token: f.token.trim() };
}
