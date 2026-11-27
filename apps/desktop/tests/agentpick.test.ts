import test from "node:test";
import assert from "node:assert/strict";
import type { AgentModels } from "../src/stores/agents/agentmodels.ts";
import { harnessFor, keepEffort, knownWorktrees, makesBranch, modelRows, railOf, startPlace } from "../src/stores/agents/agentpick.ts";

const ORDER = ["claude", "codex", "agy", "opencode", "pi", "flsh"];

test("the harness stays when the machine has it, else the machine's first by preference", () => {
  assert.equal(harnessFor("codex", ["claude", "codex"], ORDER), "codex");
  assert.equal(harnessFor("agy", ["flsh", "codex"], ORDER), "codex");
  assert.equal(harnessFor(null, ["pi", "flsh"], ORDER), "pi");
  assert.equal(harnessFor("claude", [], ORDER), null);
});

test("a thinking level carries over only to a harness that takes it", () => {
  assert.equal(keepEffort("codex", "high"), "high");
  assert.equal(keepEffort("codex", "max"), "max");
  assert.equal(keepEffort("agy", "xhigh"), "xhigh");
  assert.equal(keepEffort("agy", "max"), "max");
  assert.equal(keepEffort("agy", "off"), null);
  assert.equal(keepEffort("flsh", "low"), "low");
  assert.equal(keepEffort("flsh", "max"), null);
  assert.equal(keepEffort("opencode", "low"), null);
  assert.equal(keepEffort("claude", null), null);
});

const session = (machine_id: string, cwd: string, last_activity: number) => ({ machine_id, cwd, last_activity });

test("a checkout's worktrees come from where sessions ran, newest first", () => {
  const root = "/srv/project/widget";
  const sessions = [
    session("m1", `${root}/.worktrees/old`, 1),
    session("m1", `${root}/.worktrees/new/src`, 3),
    session("m1", `${root}/.worktrees/old`, 2),
    session("m2", `${root}/.worktrees/elsewhere`, 9),
    session("m1", root, 5),
  ];
  assert.deepEqual(knownWorktrees(sessions, "m1", `${root}/`), [`${root}/.worktrees/new`, `${root}/.worktrees/old`]);
  assert.deepEqual(knownWorktrees(sessions, null, root), []);
});

test("New branch and New worktree send a plan; the checkout and a worktree send a folder", () => {
  const cwd = "/srv/project/widget";
  assert.equal(makesBranch("branch"), true);
  assert.equal(makesBranch("new"), true);
  assert.equal(makesBranch({ worktree: `${cwd}/.worktrees/fix-it` }), false);
  assert.deepEqual(startPlace("current", cwd, "main"), { cwd, checkout: null });
  assert.deepEqual(startPlace("branch", cwd, " "), { cwd, checkout: { new_branch: { base: null } } });
  assert.deepEqual(startPlace("new", cwd, null), { cwd, checkout: { new_worktree: { base: null } } });
  assert.deepEqual(startPlace({ worktree: `${cwd}/.worktrees/fix-it` }, cwd, "main"), { cwd: `${cwd}/.worktrees/fix-it`, checkout: null });
});

test("the harness's own default entry folds into one Default row", () => {
  const claude: AgentModels = { pick: "listed", current: "default", models: [
    { id: "default", name: "Default (recommended)", description: "Opus 4.6 · Most capable for complex work", provider: null },
    { id: "sonnet", name: "Sonnet", description: null, provider: null },
  ] };
  assert.deepEqual(modelRows(claude, "").rows.map(r => [r.value, r.label, r.note]),
    [["", "Default", "Opus 4.6"], ["sonnet", "Sonnet", ""]]);
});

test("a model matches when every word of the query is in it, in any order", () => {
  const list: AgentModels = { pick: "listed", current: "", models: [
    { id: "mistral:deepseek-v4-1-flash-1017", name: "", description: null, provider: null },
    { id: "openrouter:deepseek/deepseek-chat", name: "", description: null, provider: null },
  ] };
  const ids = (q: string) => modelRows(list, q).rows.map(r => r.value).filter(Boolean);
  assert.deepEqual(ids("deepseek 1017"), ["mistral:deepseek-v4-1-flash-1017"]);
  assert.deepEqual(ids("  1017   DeepSeek "), ["mistral:deepseek-v4-1-flash-1017"]);
  assert.deepEqual(ids("deepseek"), ["mistral:deepseek-v4-1-flash-1017", "openrouter:deepseek/deepseek-chat"]);
  assert.deepEqual(ids("deepseek 9999"), []);
});

test("the pill resolves the default model when the machine reports it", async () => {
  const { modelLabel } = await import("../src/stores/agents/agentpick.ts");
  const list: AgentModels = { pick: "listed", current: "sol", models: [{ id: "sol", name: "6.1 Sol", description: null, provider: null }] };
  assert.equal(modelLabel(list, ""), "6.1 Sol");
  assert.equal(modelLabel({ pick: "listed", current: "default", models: [{ id: "default", name: "Default", description: "Opus 4.6 · Most capable", provider: null }] }, ""), "Opus 4.6");
  assert.equal(modelLabel(null, ""), "Default");
});

test("model titles resolve to IDs while labels stay readable", async () => {
  const { modelId, modelLabel } = await import("../src/stores/agents/agentpick.ts");
  const list: AgentModels = { pick: "listed", current: "model-sol", models: [
    { id: "model-sol", name: "Sol", description: null, provider: null },
  ] };
  assert.equal(modelId(list, " Sol "), "model-sol");
  assert.equal(modelId(list, "sol"), "model-sol");
  assert.equal(modelId(list, "model-sol"), "model-sol");
  assert.equal(modelId(null, "custom-model"), "custom-model");
  assert.equal(modelLabel(list, modelId(list, "Sol")), "Sol");
  list.models.push({ id: "another-sol", name: "Sol", description: null, provider: null });
  assert.equal(modelId(list, "Sol"), "Sol");
});

test("a name under several providers shows its provider and resolves with it", async () => {
  const { modelId } = await import("../src/stores/agents/agentpick.ts");
  const list: AgentModels = { pick: "listed", current: "", models: [
    { id: "openrouter:deepseek/deepseek-v4.1-flash", name: "DeepSeek V4.1 Flash", description: null, provider: "openrouter" },
    { id: "mistral:deepseek-v4-1-flash", name: "DeepSeek V4.1 Flash", description: null, provider: "mistral" },
  ] };
  assert.deepEqual(modelRows(list, "").rows.slice(1).map(r => [r.label, r.note]), [
    ["DeepSeek V4.1 Flash", "openrouter"],
    ["DeepSeek V4.1 Flash", "mistral"],
  ]);
  const ids = (q: string) => modelRows(list, q).rows.map(r => r.value).filter(Boolean);
  assert.deepEqual(ids("flash mistral"), ["mistral:deepseek-v4-1-flash"], "the provider is searched");
  assert.deepEqual(ids("deepseek v4.1"), ["openrouter:deepseek/deepseek-v4.1-flash", "mistral:deepseek-v4-1-flash"], "the name is searched");
  assert.deepEqual(ids("deepseek/"), ["openrouter:deepseek/deepseek-v4.1-flash"], "the id is searched");
  assert.deepEqual(ids("v4-1"), ids("v4.1"), "a version matches with any separator");

  assert.equal(modelId(list, "DeepSeek V4.1 Flash"), "DeepSeek V4.1 Flash", "a bare name under two providers stays as typed");
  assert.equal(modelId(list, "mistral · DeepSeek V4.1 Flash"), "mistral:deepseek-v4-1-flash");
  assert.equal(modelId(list, "openrouter/deepseek v4.1 flash"), "openrouter:deepseek/deepseek-v4.1-flash");
  assert.equal(modelId(list, "mistral:deepseek-v4-1-flash"), "mistral:deepseek-v4-1-flash");
});

test("lookup failures differ from a harness with a fixed model", () => {
  assert.equal(modelRows(null, "").note, "Couldn't load models.");
  assert.equal(modelRows({ pick: "fixed", current: null, models: [] }, "").note, "It runs its own model here.");
});

test("a search names a model, a thinking level and a harness, subscription harnesses first", async () => {
  const { modelSearch, searchedHarnesses, searchRows } = await import("../src/stores/agents/agentpick.ts");
  const offered = ["pi", "claude", "codex"];
  const claude: AgentModels = { pick: "listed", current: "default", models: [
    { id: "default", name: "Default", description: "Opus 5.5 · Most capable", provider: null },
    { id: "opus", name: "Opus 5.5", description: null, provider: null },
  ] };
  const pi: AgentModels = { pick: "listed", current: null, models: [{ id: "anthropic/claude-opus-5-5", name: "Claude Opus 5.5", description: null, provider: "anthropic" }] };
  const codex: AgentModels = { pick: "listed", current: null, models: [{ id: "gpt-6", name: "GPT-6", description: null, provider: null }] };
  const lists = new Map([["claude", claude], ["codex", codex], ["pi", pi]]);
  const rows = (q: string, picked = "codex") => {
    const search = modelSearch(q, offered);
    const harnesses = searchedHarnesses(search, picked, offered);
    return searchRows(harnesses.map(agent => ({ agent, list: lists.get(agent) })), search).rows.map(r => `${r.agent}/${r.value}${r.effort ? ` ${r.effort}` : ""}`);
  };

  assert.deepEqual(modelSearch("pi Opus 5 HIGH", offered), { agent: "pi", words: "Opus 5 HIGH" });
  assert.deepEqual(rows("opus 5 high"), ["claude/opus high", "pi/anthropic/claude-opus-5-5 high", "claude/ high"]);
  assert.deepEqual(rows("pi opus max"), ["pi/anthropic/claude-opus-5-5 max"]);
  assert.deepEqual(rows("high"), ["codex/ high", "codex/gpt-6 high"], "a level alone stays on the picked harness");
  assert.deepEqual(rows("opus 5.5 med"), ["claude/opus medium", "pi/anthropic/claude-opus-5-5 medium", "claude/ medium"]);
  assert.deepEqual(rows("claude opus 5.5 h"), ["claude/opus high", "claude/ high"], "a level matches by its start");
  assert.deepEqual(rows("claude opus m"), ["claude/opus medium", "claude/opus max", "claude/ medium", "claude/ max"], "each level a word starts shows");
  assert.deepEqual(rows("pi opus 5.5 xhi"), ["pi/anthropic/claude-opus-5-5 xhigh"]);
  assert.deepEqual(rows("pi opus 5.5 h"), ["pi/anthropic/claude-opus-5-5", "pi/anthropic/claude-opus-5-5 high"], "a word in the model also reads as a level");
  assert.ok(searchRows([{ agent: "claude", list: claude }], modelSearch("opus h", offered)).rows.some(r => r.label === "Opus 5.5 · High"));
  assert.deepEqual(rows("claude opus 5-5"), ["claude/opus", "claude/"], "a version matches with any separator");
});

test("the rail leaves out harnesses the project does not allow", () => {
  const agents = (allowed: readonly string[] | null) => railOf(["claude", "opencode"], ["claude", "opencode"], allowed).map(item => item.agent);
  assert.deepEqual(agents(["claude", "opencode"]), ["claude", "opencode"]);
  assert.deepEqual(agents([]), []);
  assert.ok(agents(null).includes("codex"));
});
