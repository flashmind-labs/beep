import test from "node:test";
import assert from "node:assert/strict";
import {
  credentialsHtml, draft, editFor, ghChoices, githubAccounts, sameServer, teaHas, teaOffers, type CliLogins, type Credential,
} from "../src/settings/credentials.ts";

const esc = (s: unknown) => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" } as Record<string, string>)[c]);
const ago = () => "today";
const providers = [
  { slug: "openrouter", name: "OpenRouter", env_var: "OPENROUTER_API_KEY" },
  { slug: "elevenlabs", name: "ElevenLabs", env_var: "ELEVENLABS_API_KEY" },
];
const list: Credential[] = [
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

test("a credential the server cannot open asks to be entered again", () => {
  const unreadable = list.map((c) => (c.id === "g" || c.id === "k" ? { ...c, unreadable: true } : c));
  const html = credentialsHtml({ list: unreadable, providers, edit: null }, machines, esc, ago);
  assert.match(html, /Re-enter this token/);
  assert.match(html, /Re-enter this key/);
  assert.doesNotMatch(html, /…a91f/);
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

test("teacli servers not in Beep can be used", () => {
  const cli: CliLogins = { github: { state: "missing" }, forgejo: [{ url: "https://git.example.com/" }, { url: "http://git.example.net:8087" }] };
  assert.deepEqual(teaOffers(cli, list).map(s => s.url), ["http://git.example.net:8087"]);
  assert.equal(teaHas(cli, "https://git.example.com"), true);
  assert.equal(teaHas(cli, "https://code.example.org"), false);
});

const accounts = [
  { id: "g1", login: "octo-one", orgs: ["acme", "acme-labs"], default: true, last4: "4f2a", created_at: 0, unreadable: false },
  { id: "g2", login: "octo-two", orgs: [], default: false, last4: "81c7", created_at: 0, unreadable: false },
];

test("without the accounts list the GitHub row of the credentials is the default", () => {
  assert.deepEqual(githubAccounts({ list }).map(a => [a.id, a.login, a.default]), [["g", "you", true]]);
  assert.deepEqual(githubAccounts({ list, github: accounts }).map(a => a.id), ["g1", "g2"]);
  assert.deepEqual(ghChoices({ github_accounts: [{ login: "OCTO-ONE" }, { login: "x" }] }, accounts).map(g => g.login), ["x"]);
});

test("server URLs compare by origin", () => {
  assert.ok(sameServer("https://git.example.com", "https://GIT.example.com:443/"));
  assert.ok(!sameServer("http://git.example.com", "https://git.example.com"));
  assert.ok(!sameServer("git.example.com", "git.example.com"));
  assert.ok(!sameServer(undefined, "https://git.example.com"));
});
