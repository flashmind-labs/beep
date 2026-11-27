import test from "node:test";
import assert from "node:assert/strict";
import {
  apiKeys, draft, editFor, ghChoices, unsavedProviders, githubAccounts, sameServer, teaHas, teaOffers, type CliLogins, type Credential,
} from "../src/stores/settings/credentials.ts";

const list: Credential[] = [
  { id: "g", kind: "github", last4: "4f2a", account: "you", env_var: "GH_TOKEN", created_at: 0 },
  { id: "f1", kind: "forgejo", url: "https://git.example.com", last4: "9c1d", account: "you", env_var: "", created_at: 0 },
  { id: "f2", kind: "forgejo", url: "https://code.example.org", last4: "02be", account: null, env_var: "", created_at: 0 },
  { id: "key.1.meshy", kind: "api_key", provider: "meshy", last4: "9f0c", account: null, env_var: "MESHY_API_KEY", created_at: 0 },
];
test("drafts carry what the server takes", () => {
  assert.deepEqual(draft({ kind: "github" }, undefined, " ghp_x ").credential,
    { kind: "github", secret: "ghp_x" });
  assert.ok(draft({ kind: "forgejo" }, "git.example.com", "t").error);
  assert.deepEqual(draft(editFor("forgejo:f1", list), undefined, "t").credential,
    { kind: "forgejo", url: "https://git.example.com", secret: "t" });
  assert.equal(editFor("api:openrouter", list), null);
  assert.ok(draft({ kind: "github" }, undefined, "  ").error);
});

test("an API key is drafted for its provider", () => {
  assert.deepEqual(draft(editFor("api_key", list), "openrouter", " sk-or-x ").credential,
    { kind: "api_key", provider: "openrouter", secret: "sk-or-x" });
  assert.deepEqual(draft(editFor("api_key:key.1.meshy", list), undefined, "msy").credential,
    { kind: "api_key", provider: "meshy", secret: "msy" });
  assert.ok(draft(editFor("api_key", list), "", "k").error);
});

test("only providers with no key are offered", () => {
  const providers = ["openrouter", "meshy", "elevenlabs"].map(p => ({ provider: p, name: p, env_var: "" }));
  const state = { list, providers };
  assert.deepEqual(apiKeys(state).map(k => k.provider), ["meshy"]);
  assert.deepEqual(unsavedProviders(state).map(p => p.provider), ["openrouter", "elevenlabs"]);
});

test("teacli servers not in Beep can be used", () => {
  const cli: CliLogins = { github: { state: "missing" }, forgejo: [{ url: "https://git.example.com/" }, { url: "http://git.example.net:3000" }] };
  assert.deepEqual(teaOffers(cli, list).map(s => s.url), ["http://git.example.net:3000"]);
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
