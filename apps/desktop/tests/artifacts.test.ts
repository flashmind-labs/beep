import test from "node:test";
import assert from "node:assert/strict";
import { artifactVersion, collectArtifacts, FRAME_SANDBOX, frameHtml, readPlace, relativeTo, toolArtifacts } from "../src/artifacts-state.ts";

const CWD = "/srv/project";
const LINK = "https://claude.ai/code/artifact/0000-placeholder";

test("an Artifact call read from Claude Code's transcript gives its file and link", () => {
  const got = toolArtifacts(`Artifact: /srv/project/docs/plan.md [completed]\n${LINK}`, CWD);
  assert.deepEqual(got.files, [{ path: "/srv/project/docs/plan.md", url: LINK, published: true }]);
  // Before its result, or from a node that does not add the link.
  assert.deepEqual(toolArtifacts("Artifact: /srv/project/docs/plan.md", CWD).files, [{ path: "/srv/project/docs/plan.md", url: null, published: true }]);
});

test("a Write of an .html file is an artifact; other files are not", () => {
  assert.deepEqual(toolArtifacts("Write: /srv/project/out/chart.html", CWD).files, [{ path: "/srv/project/out/chart.html", url: null, published: false }]);
  assert.deepEqual(toolArtifacts("Write out/chart.html [completed]\n(changed /srv/project/out/chart.html)", CWD).files,
    [{ path: "/srv/project/out/chart.html", url: null, published: false }]);
  assert.deepEqual(toolArtifacts("Write: /srv/project/src/main.rs", CWD).files, []);
  assert.deepEqual(toolArtifacts("Bash: open chart.html", CWD).files, []);
});

test("a call that failed or has not finished wrote nothing yet", () => {
  assert.deepEqual(toolArtifacts("Write: /srv/project/a.html (failed)", CWD).files, []);
  assert.deepEqual(toolArtifacts("Write a.html [in_progress]", CWD).files, []);
  assert.deepEqual(toolArtifacts("Artifact /srv/project/a.html [failed]\nerror", CWD).files, []);
});

test("artifacts come newest first, and the newest republish of a file wins", () => {
  const entries = [
    { kind: "user", text: "make a report" },
    { kind: "tool", text: "Write: /srv/project/report.html" },
    { kind: "tool", text: `Artifact: /srv/project/report.html [completed]\n${LINK}` },
    { kind: "tool", text: "Write: /tmp/agent-out/notes.md" },
    { kind: "agent", text: "Done." },
    { kind: "user", text: "make the title bigger" },
    { kind: "tool", text: "Edit /srv/project/report.html [completed]\n(changed /srv/project/report.html)" },
  ];
  const all = collectArtifacts(entries, 10, CWD);
  assert.deepEqual(all.map(a => [a.name, a.kind, a.at, a.turn, a.turnNo, a.versions, a.published, a.url]), [
    ["report.html", "html", 16, 15, 2, 3, true, LINK],
    ["notes.md", "md", 13, 10, 1, 1, false, null],
  ]);
  // Written again, its version changes, so the tab reads it again.
  const before = collectArtifacts(entries.slice(0, 5), 10, CWD)[0];
  assert.notEqual(artifactVersion(before), artifactVersion(all[0]));
});

test("a repo file the agent only edited is no artifact; a published one or one outside the folder is", () => {
  const entries = [
    { kind: "user", text: "update the docs" },
    { kind: "tool", text: "Edit /srv/project/.worktrees/docs/README.md [completed]\n(changed /srv/project/.worktrees/docs/README.md)" },
    { kind: "tool", text: "Write: /srv/project/site/index.html" },
    { kind: "tool", text: `Artifact: /srv/project/docs/plan.md [completed]\n${LINK}` },
    { kind: "tool", text: "Write: /tmp/agent-out/chart.svg" },
  ];
  assert.deepEqual(collectArtifacts(entries, 0, CWD).map(a => a.path), ["/tmp/agent-out/chart.svg", "/srv/project/docs/plan.md"]);
});

test("a file inside the session's folder is read from it; one outside from its own folder", () => {
  assert.deepEqual(readPlace("/srv/project/", "/srv/project/out/a.html"), { root: "/srv/project", path: "out/a.html", inside: true });
  assert.deepEqual(readPlace("/srv/project", "/srv/project-two/a.html"), { root: "/srv/project-two", path: "a.html", inside: false });
  assert.deepEqual(readPlace("/srv/project", "/tmp/a.html"), { root: "/tmp", path: "a.html", inside: false });
  assert.equal(relativeTo("/srv/project", "/srv/project/out/a.html"), "out/a.html");
  assert.equal(relativeTo("/srv/project", "/tmp/a.html"), null);
});

test("an HTML artifact's frame runs scripts but gets no same origin and no top navigation", () => {
  const html = frameHtml("beepview://localhost/abc", 'a "page"');
  const sandbox = /sandbox="([^"]*)"/.exec(html)?.[1];
  assert.equal(sandbox, FRAME_SANDBOX);
  const allowed = sandbox!.split(/\s+/);
  assert.ok(allowed.includes("allow-scripts"));
  for (const no of ["allow-same-origin", "allow-top-navigation", "allow-top-navigation-by-user-activation", "allow-popups", "allow-modals"]) {
    assert.ok(!allowed.includes(no), no);
  }
  assert.ok(!html.includes("srcdoc"));
  assert.match(html, /src="beepview:\/\/localhost\/abc"/);
  assert.match(html, /title="a &quot;page&quot;"/);
  assert.match(html, /referrerpolicy="no-referrer"/);
});
