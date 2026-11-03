import test from "node:test";
import assert from "node:assert/strict";
import {
  MAX_UPLOAD, type Listing, type TreeEntry, highlight, joinPath, languageOf, mentionText, parentPath, terminalName,
  treeRows, uploadParts, uploadRefusal, withMentions,
} from "../src/files-state.ts";

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

test("a folder open before it is read shows as loading", () => {
  const more = new Map(listings).set("src/auth", { entries: null, more: 0, error: "" });
  const auth = treeRows(more, new Set(["src", "src/auth"])).find(r => r.path === "src/auth")!;
  assert.ok(auth.loading);
});

test("a filter keeps matching names and the folders on the way", () => {
  assert.deepEqual(treeRows(listings, new Set(), "LOGIN").map(r => r.path), ["src", "src/login.rs"]);
  assert.deepEqual(treeRows(listings, new Set(), "nothing"), []);
});

test("paths join and split on slashes", () => {
  assert.equal(joinPath("", "a.rs"), "a.rs");
  assert.equal(joinPath("src/", "a.rs"), "src/a.rs");
  assert.equal(parentPath("src/a.rs"), "src");
  assert.equal(parentPath("a.rs"), "");
});

test("mentions go after the text, quoted with a space", () => {
  assert.equal(mentionText(["src/a.rs", ".beep/uploads/build 2.log"]), '@src/a.rs @".beep/uploads/build 2.log"');
  assert.equal(withMentions(" fix it ", ["src/a.rs"]), "fix it\n\n@src/a.rs");
  assert.equal(withMentions("", ["src/a.rs"]), "@src/a.rs");
  assert.equal(withMentions("hi", []), "hi");
});

test("uploads go in parts, an empty file as one", () => {
  assert.deepEqual(uploadParts(5, 2), [{ offset: 0, len: 2 }, { offset: 2, len: 2 }, { offset: 4, len: 1 }]);
  assert.deepEqual(uploadParts(0, 2), [{ offset: 0, len: 0 }]);
  assert.equal(uploadRefusal({ name: "a.bin", size: MAX_UPLOAD }), "");
  assert.match(uploadRefusal({ name: "a.bin", size: MAX_UPLOAD + 1 }), /over 64 MB/);
});

test("highlighting marks keywords, strings and comments, escaped", () => {
  assert.equal(highlight('let s = "<a>"; // hi', "Rust"),
    '<span class="k">let</span> s = <span class="s">&quot;&lt;a&gt;&quot;</span>; <span class="c">// hi</span>');
  assert.equal(highlight("x = 1 # note", "Python"), 'x = <span class="n">1</span> <span class="c"># note</span>');
  assert.equal(highlight("<b>", "Text"), "&lt;b&gt;");
  assert.equal(languageOf("src/main.RS"), "Rust");
  assert.equal(languageOf("Makefile"), "");
});

test("terminal tabs are named by folder and number", () => {
  assert.equal(terminalName("/srv/project/.worktrees/fix-login/", []), "fix-login 1");
  assert.equal(terminalName("/srv/fix-login", ["fix-login 1", "fix-login 3"]), "fix-login 2");
});
