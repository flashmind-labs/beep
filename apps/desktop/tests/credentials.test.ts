import test from "node:test";
import assert from "node:assert/strict";
import {
  GITHUB_TOKEN_PAGE, cliHost, credentialsHtml, draft, editFor, forgejoTokenPage, ghChoices, githubAccounts, hostOf, sameServer, teaHas, teaOffers, toolLine,
} from "../src/credentials.ts";

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

const noGithub = list.filter(c => c.kind !== "github");

test("a gh sign-in on this Mac is offered before pasting", () => {
  const cli = { github: { state: "signed_in", account: "you" }, forgejo: [] };
  const html = credentialsHtml({ list: noGithub, providers, edit: null, cli }, machines, esc, ago);
  assert.match(html, /data-cred-cli="github">Use gh sign-in/);
  assert.match(html, /gh on this Mac as @you/);
  assert.match(html, /data-cred-edit="github">Paste a token/);
  const busy = credentialsHtml({ list: noGithub, providers, edit: null, cli, cliBusy: "github" }, machines, esc, ago);
  assert.match(busy, /data-cred-cli="github" disabled>Checking…/);
  const refused = credentialsHtml({ list: noGithub, providers, edit: null, cli, cliError: { key: "github", message: "bad token" } }, machines, esc, ago);
  assert.match(refused, /class="derr"[^>]*>bad token/);
});

test("gh signed out offers to sign in with GitHub here", () => {
  const cli = { github: { state: "signed_out" }, forgejo: [] };
  const html = credentialsHtml({ list: noGithub, providers, edit: null, cli }, machines, esc, ago);
  assert.match(html, /class="dbtn pri"[^>]*data-cred-gh>Sign in with GitHub/);
  assert.match(html, /Paste a token/);
  assert.doesNotMatch(html, /in Terminal/);
  assert.doesNotMatch(html, /data-cred-cli="github"/);
  // Refused or cancelled, the reason shows and the button is back.
  const failed = credentialsHtml({ list: noGithub, providers, edit: null, cli, cliError: { key: "github", message: "Sign-in cancelled" } }, machines, esc, ago);
  assert.match(failed, /class="derr"[^>]*>Sign-in cancelled/);
  assert.match(failed, /Sign in with GitHub/);
});

test("while gh signs in the row shows the code and Cancel", () => {
  const cli = { github: { state: "signed_out" }, forgejo: [] };
  const starting = credentialsHtml({ list: noGithub, providers, edit: null, cli, ghFlow: { code: "", url: "" } }, machines, esc, ago);
  assert.match(starting, /Starting gh/);
  assert.match(starting, /data-cred-gh-cancel>Cancel/);
  assert.doesNotMatch(starting, /Sign in with GitHub/);
  const flow = { code: "AB23-77BA", url: "https://github.com/login/device", copied: true };
  const html = credentialsHtml({ list: noGithub, providers, edit: null, cli, ghFlow: flow }, machines, esc, ago);
  assert.match(html, /Enter code <code[^>]*user-select:all[^>]*>AB23-77BA<\/code> on <a[^>]*data-cred-open="https:\/\/github\.com\/login\/device"/);
  assert.match(html, /\(copied\)/);
  assert.match(html, /class="dspin"><\/span>Waiting for GitHub/);
  assert.match(html, /data-cred-gh-cancel>Cancel/);
  assert.doesNotMatch(html, /Paste a token/);
});

test("without gh the row links to GitHub's token page", () => {
  const cli = { github: { state: "missing" }, forgejo: [] };
  const html = credentialsHtml({ list: noGithub, providers, edit: null, cli }, machines, esc, ago);
  assert.match(html, /data-cred-open="https:\/\/github\.com\/settings\/tokens\/new\?scopes=repo,read:org,workflow&amp;description=Beep"/);
  assert.match(html, /Paste a token/);
  assert.match(html, /Or install GitHub CLI \(<code[^>]*>brew install gh<\/code>\) and sign in here/);
  assert.doesNotMatch(html, /data-cred-gh>/);
  // Before gh answers the paste form is still there.
  const unknown = credentialsHtml({ list: noGithub, providers, edit: null }, machines, esc, ago);
  assert.match(unknown, /data-cred-edit="github">Paste a token/);
});

test("a saved GitHub token is not offered over, unless it is unreadable", () => {
  const cli = { github: { state: "signed_in", account: "you" }, forgejo: [] };
  const saved = credentialsHtml({ list, providers, edit: null, cli }, machines, esc, ago);
  assert.doesNotMatch(saved, /Use gh sign-in/);
  const unreadable = list.map(c => (c.id === "g" ? { ...c, unreadable: true } : c));
  const html = credentialsHtml({ list: unreadable, providers, edit: null, cli }, machines, esc, ago);
  assert.match(html, /Use gh sign-in/);
  assert.match(html, /Re-enter this token/);
});

test("teacli servers not in Beep can be used", () => {
  const cli = { github: { state: "missing" }, forgejo: [{ url: "https://git.example.com/" }, { url: "http://git.example.net:8087" }] };
  assert.deepEqual(teaOffers(cli, list).map(s => s.url), ["http://git.example.net:8087"]);
  const html = credentialsHtml({ list, providers, edit: null, cli }, machines, esc, ago);
  assert.match(html, /data-cred-cli="forgejo:http:\/\/git\.example\.net:8087">Use teacli sign-in/);
  assert.equal(teaHas(cli, "https://git.example.com"), true);
  assert.equal(teaHas(cli, "https://code.example.org"), false);
});

const accounts = [
  { id: "g1", login: "octo-one", orgs: ["acme", "acme-labs"], default: true, last4: "4f2a", created_at: 0, unreadable: false },
  { id: "g2", login: "octo-two", orgs: [], default: false, last4: "81c7", created_at: 0, unreadable: false },
];

test("each GitHub account is a row, the default tagged", () => {
  const html = credentialsHtml({ list, github: accounts, providers, edit: null }, machines, esc, ago);
  assert.match(html, /@octo-one/);
  assert.match(html, /@octo-two/);
  assert.match(html, />default<\/span>/);
  assert.match(html, /orgs acme, acme-labs/);
  assert.match(html, /GH_TOKEN on your machines/);
  assert.match(html, /data-cred-gh-default="g2">Make default/);
  assert.doesNotMatch(html, /data-cred-gh-default="g1"/);
  assert.match(html, /data-cred-edit="github:g2">Replace/);
  assert.match(html, /data-cred-remove="g2"/);
  assert.match(html, /data-cred-gh-add="1">\+ Add another GitHub account/);
  // The row from the credentials list is not shown twice.
  assert.doesNotMatch(html, /data-cred-remove="g"/);
  assert.doesNotMatch(html, /undefined/);
});

test("adding another account offers gh's accounts not in Beep yet", () => {
  const cli = {
    github: { state: "signed_in", account: "octo-three" },
    github_accounts: [{ login: "octo-one", active: false }, { login: "octo-three", active: true }],
    forgejo: [],
  };
  const html = credentialsHtml({ list, github: accounts, providers, edit: null, cli, ghAdd: true }, machines, esc, ago);
  assert.match(html, /Add another GitHub account/);
  assert.match(html, /gh on this Mac as @octo-three/);
  assert.match(html, /data-cred-gh-use="octo-three">Use/);
  assert.doesNotMatch(html, /data-cred-gh-use="octo-one"/);
  assert.match(html, /data-cred-gh>Sign in with GitHub/);
  assert.match(html, /data-cred-edit="github:new">Paste a token/);
  assert.match(html, /@octo-one is in Beep already/);
  const busy = credentialsHtml({ list, github: accounts, providers, edit: null, cli, ghAdd: true, cliBusy: "gh:octo-three" }, machines, esc, ago);
  assert.match(busy, /data-cred-gh-use="octo-three" disabled>Checking…/);
  const pasting = credentialsHtml({ list, github: accounts, providers, edit: editFor("github:new", list), cli, ghAdd: true }, machines, esc, ago);
  assert.match(pasting, /Another account/);
  assert.match(pasting, /id="credSecret"/);
});

test("with no account saved, several gh accounts each get Use", () => {
  const cli = { github: { state: "signed_in", account: "octo-one" }, github_accounts: [{ login: "octo-one", active: true }, { login: "octo-two", active: false }], forgejo: [] };
  const html = credentialsHtml({ list: noGithub, github: [], providers, edit: null, cli }, machines, esc, ago);
  assert.match(html, /gh on this Mac has 2 accounts/);
  assert.match(html, /data-cred-gh-use="octo-one"/);
  assert.match(html, /data-cred-gh-use="octo-two"/);
  assert.doesNotMatch(html, /Use gh sign-in/);
});

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

test("Forgejo token pages and CLI hosts", () => {
  assert.equal(forgejoTokenPage("https://git.example.com/"), "https://git.example.com/user/settings/applications");
  assert.equal(forgejoTokenPage("https://example.com/forgejo"), "https://example.com/forgejo/user/settings/applications");
  assert.equal(forgejoTokenPage("git.example.com"), null);
  assert.equal(forgejoTokenPage("ftp://git.example.com"), null);
  assert.deepEqual(cliHost("github"), { kind: "github" });
  assert.deepEqual(cliHost("forgejo:https://git.example.com"), { kind: "forgejo", url: "https://git.example.com" });
  assert.equal(cliHost("api:openrouter"), null);
  const replacing = credentialsHtml({ list, providers, edit: editFor("forgejo:f1", list) }, machines, esc, ago);
  assert.match(replacing, /data-cred-open="https:\/\/git\.example\.com\/user\/settings\/applications"/);
  assert.match(GITHUB_TOKEN_PAGE, /^https:\/\/github\.com\/settings\/tokens\/new\?/);
});
