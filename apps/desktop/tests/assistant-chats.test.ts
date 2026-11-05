import assert from "node:assert/strict";
import { test } from "node:test";
import { keyRows } from "../src/assistant/assistant-state.ts";
import { MODELS_SHOWN, canChat, filterModels, localDate, modelLabel, webOn } from "../src/assistant/chats-state.ts";
import type { Credential } from "../src/settings/credentials.ts";

const saved = (provider: string, unreadable = false) =>
  ({ id: `api.1.${provider}`, kind: "api_key", provider, last4: "0000", unreadable, env_var: "", to_machines: true, created_at: 1, account: null }) as unknown as Credential;

test("chat shows only with a readable OpenRouter key, and Firecrawl alone turns nothing on", () => {
  assert.equal(canChat(keyRows([])), false);
  assert.equal(canChat(keyRows([saved("firecrawl")])), false);
  assert.equal(canChat(keyRows([saved("openrouter", true)])), false);
  assert.equal(canChat(keyRows([saved("openrouter")])), true);
});

test("web tools follow the Firecrawl key", () => {
  assert.equal(webOn(keyRows([saved("openrouter")])), false);
  assert.equal(webOn(keyRows([saved("openrouter"), saved("firecrawl")])), true);
});

test("the model search needs every word and keeps the listed order", () => {
  const models = [
    { id: "anthropic/claude-sonnet", name: "Claude Sonnet", tools: true },
    { id: "openai/gpt-5", name: "GPT-5", tools: true },
    { id: "anthropic/claude-haiku", name: "Claude Haiku", tools: true },
  ];
  assert.deepEqual(filterModels(models, "claude").map(m => m.id), ["anthropic/claude-sonnet", "anthropic/claude-haiku"]);
  assert.deepEqual(filterModels(models, "  anthropic SONNET ").map(m => m.id), ["anthropic/claude-sonnet"]);
  assert.equal(filterModels(models, "nothing here").length, 0);
});

test("a long model list is cut to what the picker draws", () => {
  const models = Array.from({ length: 500 }, (_, i) => ({ id: `v/m${i}`, name: `M${i}`, tools: true }));
  assert.equal(filterModels(models, "").length, MODELS_SHOWN);
});

test("the pill names the model, or the id the list lost", () => {
  const models = [{ id: "a/one", name: "One", tools: true }];
  assert.equal(modelLabel(models, "a/one"), "One");
  assert.equal(modelLabel(models, "gone/x"), "gone/x");
  assert.equal(modelLabel(null, ""), "Choose a model");
});

test("the prompt's date is the user's local day", () => {
  assert.equal(localDate(new Date(2026, 10, 5, 23, 59)), "2026-11-05");
  assert.equal(localDate(new Date(2026, 0, 2)), "2026-01-02");
});
