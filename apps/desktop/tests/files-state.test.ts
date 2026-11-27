import test from "node:test";
import assert from "node:assert/strict";
import {
  type Listing, type TreeEntry, atEdge, codeFile, failedListing, highlight, languageOf, mentionText, mergeTree, treeRows, transferSpeed, unreadFolders, uploadParts,
  withMentions,
} from "../src/stores/agentpane/files-state.ts";
import { linkedPath } from "../src/stores/agentpane/media-state.ts";

const file = (name: string, o: Partial<TreeEntry> = {}): TreeEntry => ({ name, folder: false, size: 1, ignored: false, change: null, ...o });
const dir = (name: string, o: Partial<TreeEntry> = {}): TreeEntry => file(name, { folder: true, size: 0, ...o });
const read = (entries: TreeEntry[]): Listing => ({ entries, more: 0, error: "" });

const listings = new Map<string, Listing>([
  ["", read([dir("src", { change: "modified" }), dir("target", { ignored: true }), file("README.md")])],
  ["src", read([dir("auth"), file("login.rs", { change: "modified" }), file("main.rs")])],
  ["target", read([file("app")])],
]);

test("open folders show their entries under them", () => {
  const rows = treeRows(listings, new Set(["src"]));
  assert.deepEqual(rows.map(r => `${"  ".repeat(r.depth)}${r.path}`), ["src", "  src/auth", "  src/login.rs", "  src/main.rs", "target", "README.md"]);
  assert.equal(rows[0].change, "modified");
  assert.ok(rows[0].open);
});

test("what is under an ignored folder shows as ignored", () => {
  const rows = treeRows(listings, new Set(["target"]));
  assert.deepEqual(rows.filter(r => r.ignored).map(r => r.path), ["target", "target/app"]);
});

test("a tree read merges over what was read and finds its edge", () => {
  const held = new Map(listings);
  // `src/auth` is not read; `target` is ignored, so it is no edge.
  assert.ok(atEdge(held, "src"));
  assert.ok(!atEdge(held, ""));
  assert.ok(atEdge(held, "src/auth"));
  mergeTree(held, {
    folders: [
      { path: "src", entries: [dir("auth"), file("main.rs")], more: 0 },
      { path: "src/auth", entries: [file("token.rs", { change: "added" })], more: 0 },
    ],
    truncated: false,
  });
  assert.ok(!atEdge(held, "src"));
  assert.equal(held.get("target"), listings.get("target"));
  const rows = treeRows(held, new Set(["src", "src/auth"]));
  assert.deepEqual(rows.map(r => r.path), ["src", "src/auth", "src/auth/token.rs", "src/main.rs", "target", "README.md"]);
  assert.equal(rows[2].change, "added");
});

test("a filter keeps matching names and the folders on the way", () => {
  assert.deepEqual(treeRows(listings, new Set(), "LOGIN").map(r => r.path), ["src", "src/login.rs"]);
  assert.deepEqual(treeRows(listings, new Set(), "nothing"), []);
});

test("a path filter finds folder/file and lists folder/folder", () => {
  const nested = new Map<string, Listing>([
    ["", read([dir("crates"), file("README.md")])],
    ["crates", read([dir("node")])],
    ["crates/node", read([dir("src")])],
    ["crates/node/src", read([dir("agents"), file("session_store.rs")])],
    ["crates/node/src/agents", read([dir("session"), dir("other")])],
    ["crates/node/src/agents/session", read([file("actor.rs"), file("state.rs"), dir("sub")])],
    ["crates/node/src/agents/session/sub", read([file("helper.rs")])],
    ["crates/node/src/agents/other", read([file("actor.rs")])],
  ]);

  assert.deepEqual(
    treeRows(nested, new Set(), "session/actor").map(r => r.path),
    ["crates", "crates/node", "crates/node/src", "crates/node/src/agents", "crates/node/src/agents/session", "crates/node/src/agents/session/actor.rs"],
  );
  assert.deepEqual(
    treeRows(nested, new Set(), "ses/act").map(r => r.path),
    ["crates", "crates/node", "crates/node/src", "crates/node/src/agents", "crates/node/src/agents/session", "crates/node/src/agents/session/actor.rs"],
  );
  assert.deepEqual(
    treeRows(nested, new Set(), "node/actor").map(r => r.path),
    [
      "crates", "crates/node", "crates/node/src", "crates/node/src/agents",
      "crates/node/src/agents/session", "crates/node/src/agents/session/actor.rs",
      "crates/node/src/agents/other", "crates/node/src/agents/other/actor.rs",
    ],
  );
  assert.deepEqual(
    treeRows(nested, new Set(), "agents/session").map(r => r.path),
    [
      "crates", "crates/node", "crates/node/src", "crates/node/src/agents",
      "crates/node/src/agents/session", "crates/node/src/agents/session/actor.rs",
      "crates/node/src/agents/session/state.rs", "crates/node/src/agents/session/sub",
      "crates/node/src/agents/session/sub/helper.rs",
    ],
  );
  assert.deepEqual(
    treeRows(nested, new Set(), "session/").map(r => r.path),
    [
      "crates", "crates/node", "crates/node/src", "crates/node/src/agents",
      "crates/node/src/agents/session", "crates/node/src/agents/session/actor.rs",
      "crates/node/src/agents/session/state.rs", "crates/node/src/agents/session/sub",
      "crates/node/src/agents/session/sub/helper.rs",
    ],
  );
  assert.deepEqual(
    treeRows(nested, new Set(), "./crates\\node/src/agents/session/state.rs").map(r => r.path),
    ["crates", "crates/node", "crates/node/src", "crates/node/src/agents", "crates/node/src/agents/session", "crates/node/src/agents/session/state.rs"],
  );
  assert.deepEqual(treeRows(nested, new Set(), "session/missing"), []);
});

test("a filter reads the folders nothing read yet, not what git ignores", () => {
  const read = new Map<string, Listing>([["", { entries: [dir("src"), dir("target", { ignored: true }), dir("docs")], more: 0, error: "" }], ["docs", { entries: null, more: 0, error: "no" }]]);
  assert.deepEqual(unreadFolders(read), ["src"]);
});

test("a link to a file gives its path, a web link none", () => {
  assert.deepEqual(linkedPath("/srv/app/my%20notes.md#L3"), { path: "/srv/app/my notes.md", line: 3 });
  assert.deepEqual(linkedPath("src/main.rs:12:4"), { path: "src/main.rs", line: 12 });
  assert.deepEqual(linkedPath("README.md"), { path: "README.md", line: null });
  assert.deepEqual(linkedPath("main.rs:42"), { path: "main.rs", line: 42 });
  assert.equal(linkedPath("https://example.com/a.md"), null);
  assert.equal(linkedPath("#top"), null);
});

test("inline code names a file by its folder, line or known extension", () => {
  assert.deepEqual(codeFile("src/agents/brief.rs:24"), { path: "src/agents/brief.rs", line: 24 });
  assert.deepEqual(codeFile("justfile.toml"), { path: "justfile.toml", line: null });
  assert.deepEqual(codeFile("notes.weird:3"), { path: "notes.weird", line: 3 });
  for (const text of ["self.mcp", "origin/main", "cargo test", "https://x.dev/a.rs", "v1.2"]) assert.equal(codeFile(text), null, text);
});

test("mentions go after the text, quoted with a space", () => {
  assert.equal(mentionText(["src/a.rs", ".beep/uploads/build 2.log"]), '@src/a.rs @".beep/uploads/build 2.log"');
  assert.equal(withMentions(" fix it ", ["src/a.rs"]), "fix it\n\n@src/a.rs");
  assert.equal(withMentions("", ["src/a.rs"]), "@src/a.rs");
  assert.equal(withMentions("hi", []), "hi");
});

test("image-only messages get a request before their original references", () => {
  assert.equal(withMentions("", ["/srv/shot.png"]), "Check this image.\n\n@/srv/shot.png");
  assert.equal(withMentions("  ", ["/srv/shot.png", "/srv/other.JPG"]), "Check these images.\n\n@/srv/shot.png @/srv/other.JPG");
  assert.equal(withMentions("", [], 1), "Check this image.");
  assert.equal(withMentions("", [], 2), "Check these images.");
  assert.equal(withMentions("Explain this", ["/srv/shot.png"], 1), "Explain this\n\n@/srv/shot.png");
  assert.equal(withMentions("", ["/srv/shot.png", "/srv/notes.txt"], 1), "@/srv/shot.png @/srv/notes.txt");
  assert.equal(withMentions("", []), "");
});

test("uploads go in parts, an empty file as one", () => {
  assert.deepEqual(uploadParts(5, 2), [{ offset: 0, len: 2 }, { offset: 2, len: 2 }, { offset: 4, len: 1 }]);
  assert.deepEqual(uploadParts(0, 2), [{ offset: 0, len: 0 }]);
});

test("highlighting marks keywords, strings and comments, escaped", () => {
  assert.equal(highlight('let s = "<a>"; // hi', "Rust"),
    '<span class="k">let</span> s = <span class="s">&quot;&lt;a&gt;&quot;</span>; <span class="c">// hi</span>');
  assert.equal(highlight("x = 1 # note", "Python"), 'x = <span class="n">1</span> <span class="c"># note</span>');
  assert.equal(highlight("<b>", "Text"), "&lt;b&gt;");
  assert.equal(languageOf("src/main.RS"), "Rust");
  assert.equal(languageOf("Makefile"), "");
});

test("a session folder the machine no longer has reads as empty, not as an error", () => {
  const gone = "bad request: build-box: the folder /tmp/agent/scratchpad is not there: No such file or directory (os error 2)";
  assert.deepEqual(failedListing(gone), { entries: [], more: 0, error: "" });
  assert.equal(failedListing("bad request: build-box: permission denied").error, "build-box: permission denied");
  assert.match(failedListing("unknown op 812").error, /Update beep-node/);
});

test("mergeTree reports whether a read changed anything", () => {
  const held = new Map<string, Listing>();
  const tree = (names: string[]) => ({
    truncated: false,
    folders: [{ path: "", more: 0, entries: names.map(name => ({ name, folder: false, size: 1, ignored: false, change: null })) }],
  });
  assert.equal(mergeTree(held, tree(["a"])), true);
  assert.equal(mergeTree(held, tree(["a"])), false);
  assert.equal(mergeTree(held, tree(["a", "b"])), true);
});

test("a transfer shows its speed once past five seconds and under 80%", () => {
  const mb = 1024 * 1024;
  assert.equal(transferSpeed({ started: 0, done: 10 * mb, size: 100 * mb }, 4000), null);
  assert.equal(transferSpeed({ started: 0, done: 10 * mb, size: 100 * mb }, 10_000), "1 MB/s");
  assert.equal(transferSpeed({ started: 0, done: 80 * mb, size: 100 * mb }, 10_000), null);
});
