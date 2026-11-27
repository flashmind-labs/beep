import test from "node:test";
import assert from "node:assert/strict";
import {
  IDLE, UNEXPECTED, consentAction, consentTitle, consentVersions, flowStep, isNewPermission, openStage, parseFailure, parsePreview,
  parseStaged, parseUpdateCheck, permissionLabel, permissionMeaning, repositoryName, rowOffers, runLine, sourceLabel, sourceSentence,
  updateLine, type Flow, type PluginPreview, type StagedPlugin,
} from "../src/stores/settings/plugins-settings-state.ts";
import { parseRows, parseSource, pluginSkins, type PluginRow } from "../src/ui/plugin-rows.ts";

const git = { kind: "git", url: "https://github.com/owner/repo.git", ref: null, subfolder: null, commit: "4f0915a8b3c2d1e0f9a8b7c6d5e4f30211223344" };
const raw = (extra: Record<string, unknown> = {}) => ({
  id: "composer-cat", name: "Composer cat", version: "0.2.0", description: "A cat.", permissions: ["ui.add"], approved: ["ui.add"],
  source: git, enabled: true, state: "running", ...extra,
});
const row = (extra: Partial<PluginRow> = {}): PluginRow => {
  const parsed = parseRows([raw()]);
  assert.ok(parsed);
  return { ...parsed[0], ...extra };
};
const preview = (extra: Partial<PluginPreview> = {}): PluginPreview => ({
  id: "composer-cat", name: "Composer cat", version: "0.2.0", description: "A cat.", permissions: ["ui.add"], source: { kind: "git", url: git.url, ref: null, subfolder: null, commit: git.commit },
  kind: "new", previousVersion: null, added: ["ui.add"], removed: [], ...extra,
});
const staged = (extra: Partial<PluginPreview> = {}): StagedPlugin => ({ stage: "s-1", preview: preview(extra) });

test("a status row is read, with its source, state and permissions", () => {
  const rows = parseRows([raw(), raw({ id: "notes", state: "needs-approval", missing: ["sessions.read"], permissions: ["ui.add", "sessions.read"], source: { kind: "local" } })]);
  assert.ok(rows);
  assert.equal(rows.length, 2);
  assert.deepEqual(rows[0].run, { state: "running" });
  assert.deepEqual(rows[0].source, { kind: "git", url: git.url, ref: null, subfolder: null, commit: git.commit });
  assert.deepEqual(rows[1].run, { state: "needs-approval", missing: ["sessions.read"] });
  assert.deepEqual(rows[1].source, { kind: "local" });
});

test("every state the host reports is read", () => {
  for (const state of ["waiting", "running", "paused", "disabled"]) assert.deepEqual(parseRows([raw({ state })])?.[0].run, { state });
  assert.deepEqual(parseRows([raw({ state: "stopped", reason: "used 600 ms" })])?.[0].run, { state: "stopped", reason: "used 600 ms" });
});

test("a row that is not what the desktop promised makes the list unreadable", () => {
  for (const bad of [
    raw({ state: "exploded" }), raw({ state: "stopped" }), raw({ state: "needs-approval" }), raw({ permissions: ["fs.read"] }), raw({ approved: "ui.add" }),
    raw({ source: { kind: "ftp" } }), raw({ source: { kind: "git", url: 1 } }), raw({ enabled: "yes" }), raw({ version: 2 }), null, "text",
  ]) assert.equal(parseRows([bad]), null, JSON.stringify(bad));
  assert.equal(parseRows({}), null);
  assert.equal(parseRows(null), null);
  assert.deepEqual(parseRows([]), []);
});

test("a git source keeps its branch and folder", () => {
  assert.deepEqual(parseSource({ ...git, ref: "main", subfolder: "plugins/cat" }), { ...git, ref: "main", subfolder: "plugins/cat" });
  assert.deepEqual(parseSource({ kind: "dev" }), { kind: "dev" });
  assert.equal(parseSource({ kind: "git", url: "x" }), null);
});

test("sources read as places", () => {
  assert.equal(sourceLabel({ kind: "local" }), "Local folder");
  assert.equal(sourceLabel({ kind: "dev" }), "Dev folder");
  assert.equal(sourceLabel(git as never), "github.com/owner/repo at 4f0915a");
  assert.equal(sourceLabel({ ...git, subfolder: "plugins/cat" } as never), "github.com/owner/repo, plugins/cat at 4f0915a");
  assert.equal(sourceSentence(git as never), "From github.com/owner/repo, commit 4f0915a");
  assert.equal(sourceSentence({ kind: "local" }), "From a folder on this computer");
});

test("repository addresses lose their scheme, user and .git", () => {
  assert.equal(repositoryName("https://github.com/owner/repo.git"), "github.com/owner/repo");
  assert.equal(repositoryName("ssh://git@git.example.test/team/plugins.git"), "git.example.test/team/plugins");
  assert.equal(repositoryName("git@github.com:owner/repo.git"), "github.com/owner/repo");
  assert.equal(repositoryName("https://git.example.test:8443/team/plugins"), "git.example.test:8443/team/plugins");
});

test("permissions have plain labels", () => {
  assert.equal(permissionLabel("ui.add"), "Add to the interface");
  assert.equal(permissionLabel("sessions.read"), "Read your sessions");
  assert.equal(permissionLabel("tools"), "Give tools to your agents");
  assert.equal(permissionMeaning("tools"), "Coding agents can call the tools this plugin lists.");
});

test("the state line says what the user sees", () => {
  assert.deepEqual(runLine({ state: "running" }, false), { text: "On", tone: "ok" });
  assert.deepEqual(runLine({ state: "disabled" }, true), { text: "Off", tone: "off" });
  assert.deepEqual(runLine({ state: "needs-approval", missing: ["ui.add"] }, false), { text: "Needs approval", tone: "warn" });
  assert.deepEqual(runLine({ state: "waiting" }, false), { text: "Waiting for its slot", tone: "off" });
  assert.deepEqual(runLine({ state: "waiting" }, true), { text: "Pick it in Appearance", tone: "ok" });
  assert.deepEqual(runLine({ state: "paused" }, false), { text: "Paused", tone: "off" });
  assert.deepEqual(runLine({ state: "stopped", reason: "used 600 ms" }, false), { text: "Stopped: used 600 ms", tone: "err" });
});

test("a row offers update for git plugins and allow for plugins that need approval", () => {
  assert.deepEqual(rowOffers(row()), { on: true, switchLocked: false, update: true, allow: false, remove: true });
  assert.deepEqual(rowOffers(row({ source: { kind: "local" }, enabled: false, run: { state: "disabled" } })), { on: false, switchLocked: false, update: false, allow: false, remove: true });
  assert.equal(rowOffers(row({ source: { kind: "builtin" } })).remove, false);
  const waiting = rowOffers(row({ run: { state: "needs-approval", missing: ["ui.add"] } }));
  assert.equal(waiting.on, false, "a plugin that needs approval shows off");
  assert.equal(waiting.switchLocked, true);
  assert.equal(waiting.allow, true);
});

test("the consent sheet names the action and the versions", () => {
  assert.equal(consentTitle(preview()), "Install Composer cat?");
  assert.equal(consentAction(preview()), "Allow and install");
  assert.equal(consentVersions(preview()), "Composer cat 0.2.0");

  const update = preview({ kind: "update", previousVersion: "0.1.0", added: ["sessions.read"], permissions: ["ui.add", "sessions.read"] });
  assert.equal(consentTitle(update), "Update Composer cat?");
  assert.equal(consentAction(update), "Allow and update");
  assert.equal(consentVersions(update), "Composer cat 0.1.0 to 0.2.0");
  assert.equal(isNewPermission(update, "sessions.read"), true);
  assert.equal(isNewPermission(update, "ui.add"), false);
  assert.equal(isNewPermission(preview(), "ui.add"), false, "a first install lists everything without flagging it");

  const allow = preview({ kind: "allow" });
  assert.equal(consentTitle(allow), "Allow Composer cat?");
  assert.equal(consentAction(allow), "Allow");
});

test("previews, staged plugins and update checks are read", () => {
  const wire = { ...preview(), source: git };
  assert.deepEqual(parsePreview(wire)?.kind, "new");
  assert.deepEqual(parseStaged({ stage: "s-1", preview: wire })?.stage, "s-1");
  assert.equal(parseStaged({ stage: 1, preview: wire }), null);
  assert.equal(parsePreview({ ...wire, kind: "other" }), null);
  assert.equal(parsePreview({ ...wire, added: ["fs.read"] }), null);
  assert.deepEqual(parseUpdateCheck({ state: "available", commit: "abc1234" }), { state: "available", commit: "abc1234" });
  assert.deepEqual(parseUpdateCheck({ state: "pinned" }), { state: "pinned" });
  assert.equal(parseUpdateCheck({ state: "available" }), null);
  assert.equal(parseUpdateCheck("current"), null);
});

test("update lines read as sentences", () => {
  assert.equal(updateLine({ kind: "checking" }), "Checking for an update…");
  assert.equal(updateLine({ kind: "current" }), "Up to date");
  assert.equal(updateLine({ kind: "failed", message: "Git is not installed." }), "Git is not installed.");
  assert.match(updateLine({ kind: "pinned" }), /fixed commit/);
});

test("typed failures keep their message and anything else becomes one", () => {
  assert.deepEqual(parseFailure({ kind: "git", message: "Git is not installed." }), { kind: "git", message: "Git is not installed." });
  assert.deepEqual(parseFailure("no answer in 120 s. Try again."), { kind: "unknown", message: "no answer in 120 s. Try again." });
  assert.deepEqual(parseFailure(new Error("boom")), { kind: "unknown", message: "boom" });
  assert.equal(parseFailure(null).message, "Something went wrong. Try again.");
  assert.equal(parseFailure({ kind: 3 }).kind, "unknown");
});

test("the Git form fills in, checks and fetches", () => {
  let flow: Flow = flowStep(IDLE, { type: "open-git" });
  assert.deepEqual(flow, { kind: "git", repository: "", reference: "", folder: "", failure: null });

  const empty = flowStep(flow, { type: "submit-git" });
  assert.equal(empty.kind, "git");
  assert.match(empty.kind === "git" && empty.failure ? empty.failure.message : "", /Enter a repository/);

  flow = flowStep(empty, { type: "edit-git", field: "repository", value: "owner/repo" });
  assert.equal(flow.kind === "git" && flow.failure, null, "typing clears the complaint");
  flow = flowStep(flow, { type: "edit-git", field: "reference", value: "v1" });
  flow = flowStep(flow, { type: "edit-git", field: "folder", value: "plugins/cat" });
  assert.deepEqual(flowStep(flow, { type: "submit-git" }), { kind: "fetching", repository: "owner/repo", reference: "v1", folder: "plugins/cat" });
});

test("a failed fetch returns to the form with the typed message and what was typed", () => {
  const fetching: Flow = { kind: "fetching", repository: "owner/missing", reference: "", folder: "" };
  const failure = { kind: "git", message: "Git could not read that repository." };

  assert.deepEqual(flowStep(fetching, { type: "failed", failure }), { kind: "git", repository: "owner/missing", reference: "", folder: "", failure });
});

test("a fetched plugin opens the consent sheet, and confirming works through busy and back", () => {
  const fetching: Flow = { kind: "fetching", repository: "owner/repo", reference: "", folder: "" };
  let flow = flowStep(fetching, { type: "staged", staged: staged() });
  assert.equal(flow.kind, "consent");
  assert.equal(openStage(flow), "s-1");

  flow = flowStep(flow, { type: "confirming" });
  assert.equal(flow.kind === "consent" && flow.busy, true);

  const failure = { kind: "already-installed", message: "A plugin with the id composer-cat is already installed from another source. Remove it first." };
  flow = flowStep(flow, { type: "failed", failure });
  assert.equal(flow.kind === "consent" && flow.busy, false, "the sheet can be tried again or cancelled");
  assert.deepEqual(flow.kind === "consent" && flow.failure, failure);

  assert.deepEqual(flowStep(flow, { type: "closed" }), IDLE);
});

test("allowing an installed plugin has no stage to throw away", () => {
  const flow = flowStep(IDLE, { type: "reviewed", preview: preview({ kind: "allow" }) });

  assert.equal(flow.kind, "consent");
  assert.equal(openStage(flow), null);
  assert.equal(openStage(IDLE), null);
});

test("reading a folder is one step that fails or ends in consent", () => {
  const working = flowStep(IDLE, { type: "work" });
  assert.deepEqual(working, { kind: "working" });
  assert.equal(flowStep(flowStep(IDLE, { type: "open-git" }), { type: "work" }).kind, "git", "a form in use is not replaced");

  const failure = { kind: "no-manifest", message: "There is no beep-plugin.json in the repository." };
  assert.deepEqual(flowStep(working, { type: "failed", failure }), { kind: "idle", failure });
  assert.equal(flowStep(working, { type: "staged", staged: staged({ source: { kind: "local" } }) }).kind, "consent");
});

test("events that do not fit the flow change nothing", () => {
  assert.deepEqual(flowStep(IDLE, { type: "edit-git", field: "repository", value: "x" }), IDLE);
  assert.deepEqual(flowStep(IDLE, { type: "submit-git" }), IDLE);
  assert.deepEqual(flowStep(IDLE, { type: "confirming" }), IDLE);
  assert.equal(UNEXPECTED.kind, "unexpected");
});

test("a plugin is a skin while it sets tokens, is on and may restyle the app", () => {
  const row = (id: string, extra: Record<string, unknown>) => ({
    id, name: id.toUpperCase(), version: "0.1.0", description: "", permissions: ["style"], approved: ["style"],
    source: { kind: "local" }, enabled: true, state: "waiting", style: { "--radius": "0", "--surface": "#123" }, ...extra,
  });
  const rows = parseRows([
    row("flat", {}),
    row("off", { enabled: false, state: "disabled" }),
    row("asks", { approved: [], state: "needs-approval", missing: ["style"] }),
    row("plain", { style: {}, permissions: [], approved: [] }),
  ]);

  assert.deepEqual(pluginSkins(rows ?? []).map(skin => [skin.id, skin.name, skin.swatch, [...skin.tokens]]), [["flat", "FLAT", "#123", [["--radius", "0"], ["--surface", "#123"]]]]);
  assert.equal(parseRows([row("bad", { style: { color: "red" } })]), null);
});
