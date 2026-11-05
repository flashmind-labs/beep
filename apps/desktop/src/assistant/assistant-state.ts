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

/// The keys the assistant takes, all optional.
export const KEY_PROVIDERS: readonly KeyProvider[] = [
  { slug: "openrouter", name: "OpenRouter", use: "Chat", url: "https://openrouter.ai/keys" },
  { slug: "firecrawl", name: "Firecrawl", use: "Web search", url: "https://www.firecrawl.dev/app/api-keys" },
  { slug: "openai", name: "OpenAI", use: "Dictation", url: "https://platform.openai.com/api-keys" },
  { slug: "mistral", name: "Mistral", use: "Dictation", url: "https://console.mistral.ai/api-keys" },
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
