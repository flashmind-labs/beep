import assert from "node:assert/strict";
import { test } from "node:test";
import { blankMcpForm, KEYS_NOTE, keyRows, MCP_NOTE, type McpRow, mcpDetail, mcpFormReady, mcpStatus, newKey, newMcpServer } from "../src/assistant/assistant-state.ts";
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

test("an MCP row shows its tools, or why it is not, and never a secret", () => {
  const row = (status: McpRow["status"], over: Partial<McpRow> = {}): McpRow => ({ name: "files", detail: "tool --flag", env: [], token: false, status, ...over });
  assert.deepEqual(mcpStatus(row({ state: "ready", tools: 6 })), { text: "6 tools", failed: false });
  assert.deepEqual(mcpStatus(row({ state: "ready", tools: 1 })), { text: "1 tool", failed: false });
  assert.deepEqual(mcpStatus(row({ state: "idle" })), { text: "Not started", failed: false });
  assert.deepEqual(mcpStatus(row({ state: "failed", error: "x" })), { text: "Failed", failed: true });
  assert.equal(mcpDetail(row({ state: "idle" })), "tool --flag");
  assert.equal(mcpDetail(row({ state: "idle" }, { env: ["API_KEY"], token: true })), "tool --flag · API_KEY, token set");
  assert.doesNotMatch(MCP_NOTE, /[—→⇒←]/);
});

test("the add form sends only what its kind uses, trimmed", () => {
  const f = { ...blankMcpForm(), name: " files ", command: " npx -y x ", env: "KEY=v", url: "https://ignored.test", token: "ignored" };
  assert.equal(mcpFormReady(f), true);
  assert.deepEqual(newMcpServer(f), { kind: "command", name: "files", command: "npx -y x", env: "KEY=v" });
  const u = { ...f, kind: "url" as const, url: " https://example.test/mcp ", token: " tok " };
  assert.deepEqual(newMcpServer(u), { kind: "url", name: "files", url: "https://example.test/mcp", token: "tok" });
  assert.equal(mcpFormReady({ ...blankMcpForm(), name: "x" }), false);
});
