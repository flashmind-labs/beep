import assert from "node:assert/strict";
import { test } from "node:test";
import { KEYS_NOTE, keyRows, newKey } from "../src/assistant/assistant-state.ts";
import type { Credential } from "../src/settings/credentials.ts";

const saved = (provider: string, last4: string, unreadable = false) =>
  ({ id: `api.1.${provider}`, kind: "api_key", provider, last4, unreadable, env_var: "", to_machines: true, created_at: 1, account: null }) as unknown as Credential;

test("the page lists OpenRouter and Firecrawl with no key saved, and shows last four once saved", () => {
  assert.deepEqual(keyRows(null).map(r => [r.provider.slug, r.saved]), [["openrouter", null], ["firecrawl", null]]);
  const rows = keyRows([saved("firecrawl", "4d2f"), saved("meshy", "0000")]);
  assert.equal(rows[0].saved, null);
  assert.deepEqual(rows[1].saved, { id: "api.1.firecrawl", last4: "4d2f", unreadable: false });
});

test("a key saved here goes to the machines, trimmed", () => {
  const [openrouter] = keyRows(undefined);
  assert.deepEqual(newKey(openrouter.provider, "  sk-or-v1-abc \n"), { kind: "api_key", provider: "openrouter", secret: "sk-or-v1-abc", to_machines: true });
});

test("the page says who is paid and where the keys go, in plain words", () => {
  assert.doesNotMatch(KEYS_NOTE, /[—→⇒←]/);
  assert.match(KEYS_NOTE, /available to commands on your machines/);
});
