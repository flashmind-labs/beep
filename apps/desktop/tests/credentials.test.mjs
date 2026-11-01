import test from "node:test";
import assert from "node:assert/strict";
import { credentialsHtml, draft, editFor, hostOf, toolLine } from "../dist/credentials.js";

const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const ago = () => "today";
const providers = [
  { slug: "openrouter", name: "OpenRouter", env_var: "OPENROUTER_API_KEY" },
  { slug: "elevenlabs", name: "ElevenLabs", env_var: "ELEVENLABS_API_KEY" },
];
const list = [
  { id: "g", kind: "github", last4: "4f2a", account: "you", env_var: "GH_TOKEN", to_machines: true, created_at: 0 },
  { id: "f1", kind: "forgejo", url: "https://git.example.com", last4: "9c1d", account: "you", env_var: "", to_machines: true, created_at: 0 },
  { id: "f2", kind: "forgejo", url: "https://code.example.org", last4: "02be", account: null, env_var: "", to_machines: true, created_at: 0 },
  { id: "k", kind: "api_key", provider: "openrouter", last4: "a91f", account: null, env_var: "OPENROUTER_API_KEY", to_machines: true, created_at: 0 },
];
const machines = [
  { name: "build-box", env: { tools: [{ name: "gh" }, { name: "teacli" }] } },
  { name: "mac-box", env: { tools: [{ name: "gh" }] } },
  { name: "gpu-box", env: null },
];

test("each Forgejo server is its own row, with an add button", () => {
  const html = credentialsHtml({ list, providers, edit: null }, machines, esc, ago);
  assert.match(html, /git\.example\.com/);
  assert.match(html, /code\.example\.org/);
  assert.match(html, /\+ Add a Forgejo server/);
  assert.match(html, /gh on build-box, mac-box/);
  assert.match(html, /teacli on build-box/);
  assert.match(html, /…4f2a/);
  assert.match(html, /@you/);
  // A key that is missing gets an Add button and shows its env var.
  assert.match(html, /data-cred-edit="api:elevenlabs">Add key/);
  assert.match(html, /ELEVENLABS_API_KEY/);
  assert.doesNotMatch(html, /undefined/);
  // API keys go to commands and terminals, never to agents.
  assert.match(html, /Agents do not get API keys/);
  assert.doesNotMatch(html, /run agents/);
});

test("a credential the server cannot open asks to be entered again", () => {
  const unreadable = list.map((c) => (c.id === "g" || c.id === "k" ? { ...c, unreadable: true } : c));
  const html = credentialsHtml({ list: unreadable, providers, edit: null }, machines, esc, ago);
  assert.match(html, /Re-enter this token/);
  assert.match(html, /Re-enter this key/);
  assert.doesNotMatch(html, /…4f2a/);
  assert.doesNotMatch(html, /…a91f/);
});

test("without the machines list only the token shows", () => {
  const html = credentialsHtml({ list, providers, edit: null }, null, esc, ago);
  assert.doesNotMatch(html, / on build-box/);
  assert.equal(toolLine("gh", null), "");
  assert.equal(toolLine("teacli", []), "teacli on no machine yet");
});

test("drafts carry what the server takes", () => {
  assert.deepEqual(draft({ kind: "github" }, undefined, " ghp_x ", false).credential,
    { kind: "github", secret: "ghp_x", to_machines: true });
  assert.ok(draft({ kind: "forgejo" }, "git.example.com", "t").error);
  assert.deepEqual(draft(editFor("forgejo:f1", list), undefined, "t").credential,
    { kind: "forgejo", url: "https://git.example.com", secret: "t", to_machines: true });
  assert.deepEqual(draft(editFor("api:openrouter", list), undefined, "sk", false).credential,
    { kind: "api_key", provider: "openrouter", secret: "sk", to_machines: false });
  assert.ok(draft({ kind: "github" }, undefined, "  ").error);
});

test("adding a server shows the URL field, replacing one does not", () => {
  const adding = credentialsHtml({ list, providers, edit: editFor("forgejo", list) }, machines, esc, ago);
  assert.match(adding, /id="credUrl"/);
  assert.doesNotMatch(adding, /\+ Add a Forgejo server/);
  const replacing = credentialsHtml({ list, providers, edit: editFor("forgejo:f2", list) }, machines, esc, ago);
  assert.doesNotMatch(replacing, /id="credUrl"/);
  assert.match(replacing, /id="credSecret"/);
  assert.equal(hostOf("https://git.example.com/"), "git.example.com");
});
