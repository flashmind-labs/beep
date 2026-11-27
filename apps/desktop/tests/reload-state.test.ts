import test from "node:test";
import assert from "node:assert/strict";
import type { Tree } from "../src/stores/agentpane/files-state.ts";
import { followedAt, folderStamp, hashedStamp, listedSize, nextStamp, splitPath, textStamp } from "../src/stores/agentpane/reload-state.ts";

const entry = (name: string, size: number, folder = false) => ({ name, folder, size, ignored: false, change: null });
const tree = (folders: Tree["folders"]): Tree => ({ folders, truncated: false });

test("a file is followed in the first watched folder holding it, else in its own folder", () => {
  assert.deepEqual(followedAt(["/srv/project/.worktrees/a", "/srv/project"], "/srv/project/out/chart.png"), { root: "/srv/project", path: "out/chart.png" });
  assert.deepEqual(followedAt(["/srv/project/.worktrees/a", "/srv/project"], "/srv/project/.worktrees/a/x.pdf"), { root: "/srv/project/.worktrees/a", path: "x.pdf" });
  assert.deepEqual(followedAt(["", "/srv/project"], "/work/report.html"), { root: "/work", path: "report.html" });
  assert.deepEqual(splitPath("out/site/index.html"), { dir: "out/site", name: "index.html" });
  assert.deepEqual(splitPath("index.html"), { dir: "", name: "index.html" });
});

test("a file's size comes from its own folder's listing, not a namesake elsewhere", () => {
  const listing = tree([
    { path: "out", entries: [entry("chart.png", 120), entry("chart", 0, true)], more: 0 },
    { path: "out/chart", entries: [entry("chart.png", 999)], more: 0 },
  ]);
  assert.equal(listedSize(listing, "out", "chart.png"), 120);
  assert.equal(listedSize(listing, "out", "chart"), null);
  // A file being replaced may be missing for a moment.
  assert.equal(listedSize(listing, "out", "gone.png"), null);
});

test("a page's folder stamp changes with the files under it, not with git's marks", () => {
  const before = tree([
    { path: "site", entries: [entry("index.html", 40), entry("css", 0, true)], more: 0 },
    { path: "site/css", entries: [entry("style.css", 300)], more: 0 },
  ]);
  const staged = tree(before.folders.map(f => ({ ...f, entries: f.entries.map(e => ({ ...e, change: "added" as const })) })));
  const edited = tree([before.folders[0], { ...before.folders[1], entries: [entry("style.css", 310)] }]);
  assert.equal(folderStamp(staged), folderStamp(before));
  assert.notEqual(folderStamp(edited), folderStamp(before));
  // Text is compared whole: an edit that keeps the size still shows.
  const text = (body: string) => ({ size: body.length, text: body, cut: false, binary: false, editable: true });
  assert.notEqual(textStamp(text("color: #fff")), textStamp(text("color: #000")));
});

test("a page changes when it or a file it loaded hashes differently, not when it loads one more", () => {
  const hashes = new Map([["site/index.html", "h1"], ["site/style.css", "c1"]]);
  const before = hashedStamp("site/index.html", ["site/style.css"], hashes);
  assert.ok(before);
  // The page being replaced has no hash for a moment: nothing to compare.
  assert.equal(hashedStamp("site/index.html", [], new Map([["site/style.css", "c1"]])), null);

  const more = hashedStamp("site/index.html", ["site/style.css", "site/app.js"], new Map([...hashes, ["site/app.js", "j1"]]));
  assert.ok(more);
  const grown = nextStamp(before, more);
  assert.equal(grown.changed, false);
  assert.deepEqual([...grown.stamp.keys()], ["site/index.html", "site/style.css", "site/app.js"]);

  const restyled = hashedStamp("site/index.html", ["site/style.css"], new Map([["site/index.html", "h1"], ["site/style.css", "c2"]]));
  assert.ok(restyled);
  assert.equal(nextStamp(grown.stamp, restyled).changed, true);
  // A loaded file missing for now keeps its last hash, so its return is compared.
  const kept = nextStamp(grown.stamp, new Map([["site/index.html", "h1"]]));
  assert.equal(kept.changed, false);
  assert.equal(kept.stamp.get("site/app.js"), "j1");
});
