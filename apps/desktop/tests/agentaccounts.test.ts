import test from "node:test";
import assert from "node:assert/strict";
import { agentAccountsHtml, machinesWith, ownLogin, timeLeft, tokenDraft, usedOn, type CliLogin } from "../src/agentaccounts.ts";

const esc = (s: unknown) => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" } as Record<string, string>)[c]);
const ago = () => "today";
const machines = [
  { id: "a", name: "build-box", online: true, claude: true, codex: true, claude_sessions: 2, codex_signed_in: true },
  { id: "b", name: "Workspace", online: true, claude: true, codex: true, claude_sessions: 0, codex_signed_in: false },
  { id: "c", name: "gpu-box", online: false, claude: true, codex: true, claude_sessions: 0, codex_signed_in: null },
  { id: "d", name: "printer", online: true, claude: false, codex: false, claude_sessions: 0, codex_signed_in: null },
];
const signedIn = { claude: { last4: "7d1e", created_at: 0, unreadable: false }, codex_key: null, machines };

test("each machine says how it stands with the Claude token", () => {
  assert.equal(usedOn(machines[0]), "2 sessions now");
  assert.equal(usedOn({ ...machines[0], claude_sessions: 1 }), "1 session now");
  assert.equal(usedOn(machines[1]), "idle");
  assert.equal(usedOn(machines[2]), "offline · gets it when back");
  assert.deepEqual(machinesWith(machines, "claude").map(m => m.name), ["build-box", "Workspace", "gpu-box"]);
});

test("signed in: Replace, Revoke and the machines it is used on", () => {
  const html = agentAccountsHtml({ data: signedIn, codex: {} }, esc, ago);
  assert.match(html, /signed in today/);
  assert.match(html, /…7d1e/);
  assert.match(html, /data-ag-revoke="claude"/);
  assert.match(html, /2 sessions now/);
  assert.match(html, /offline · gets it when back/);
  assert.doesNotMatch(html, /printer/);
  // Codex: signed in, a Sign in button, offline.
  assert.match(html, /data-ag-codex-sign="b"/);
  assert.doesNotMatch(html, /data-ag-codex-sign="a"/);
  assert.match(html, /Use an API key/);
});

test("not signed in: Sign in for all, or paste a token", () => {
  const html = agentAccountsHtml({ data: { ...signedIn, claude: null }, codex: {} }, esc, ago);
  assert.match(html, /data-ag-claude-sign>Sign in for all</);
  assert.match(html, /data-ag-edit="claude"/);
  assert.doesNotMatch(html, /Used on/);
  const typing = agentAccountsHtml({ data: { ...signedIn, claude: null }, codex: {}, edit: "claude" }, esc, ago);
  assert.match(typing, /id="agSecret"/);
});

test("a Codex device code shows with the time left", () => {
  const now = 1_000_000;
  const codex = { b: { task: 4, url: "https://auth.openai.com/codex/device", code: "K7QD-M2XR", expires_at: now + (14 * 60 + 32) * 1000 } };
  const html = agentAccountsHtml({ data: signedIn, codex }, esc, ago, now);
  assert.match(html, /auth\.openai\.com\/codex\/device/);
  assert.match(html, /K7QD-M2XR/);
  assert.match(html, /14:32 left/);
  assert.match(html, /data-ag-copy="K7QD-M2XR"/);
  assert.match(html, /data-ag-codex-cancel="b"/);
  assert.equal(timeLeft(now - 1, now), "expired");
});

test("a saved Codex key and a server too old", () => {
  const html = agentAccountsHtml({ data: { ...signedIn, codex_key: { last4: "9f0a", created_at: 0, unreadable: false } }, codex: {} }, esc, ago);
  assert.match(html, /API key <code[^>]*>…9f0a<\/code> on every machine/);
  assert.match(html, /data-ag-revoke="codex"/);
  assert.match(agentAccountsHtml({ data: false }, esc, ago), /needs an update/);
  assert.match(agentAccountsHtml({ data: null }, esc, ago), /Could not load/);
});

test("a pasted token is checked before it is sent", () => {
  assert.deepEqual(tokenDraft("claude", " sk-ant-oat01-abc \n"), { agent: "claude", secret: "sk-ant-oat01-abc" });
  assert.match(tokenDraft("claude", "sk-ant-api03-abc").error!, /API key/);
  assert.match(tokenDraft("codex", "").error!, /key/);
  assert.match(tokenDraft("codex", "a b").error!, /spaces/);
  assert.equal(tokenDraft(null, "x").error, "Nothing to save.");
});

test("without Beep's token, Claude Code shows each machine's own sign-in", () => {
  assert.equal(ownLogin(undefined), "not checked");
  assert.equal(ownLogin({ tool: "claude", signed_in: false }), "not signed in");
  assert.equal(ownLogin({ tool: "claude", signed_in: true, account: "you@example.com" }), "signed in as you@example.com");
  const machine = { online: true, claude: true, codex: false, claude_sessions: 0, codex_signed_in: null };
  const data = {
    claude: null,
    codex_key: null,
    machines: [{ ...machine, id: "m1", name: "This Mac" }, { ...machine, id: "m2", name: "Workspace" }],
    claude_logins: new Map<string, CliLogin>([["m1", { tool: "claude", signed_in: true, account: null }], ["m2", { tool: "claude", signed_in: false }]]),
  };
  const html = agentAccountsHtml({ data, codex: {} }, s => String(s), () => "now");
  assert.match(html, /This Mac.*signed in/s);
  assert.match(html, /Workspace.*not signed in/s);
  assert.match(html, /Sign in for all/);
});
