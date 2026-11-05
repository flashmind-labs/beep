// Run as `node --import ./tests/ts.mjs --test tests/*.test.ts`.
import test from "node:test";
import assert from "node:assert/strict";
import { type AgentCommand, commandText, filePaths, filterCommands, filterFiles, indexFiles, menuKey, picked, triggerAt } from "../src/agents/mention-state.ts";

const end = (text: string) => triggerAt(text, text.length);
const keys = { shift: false, alt: false, ctrl: false, meta: false, composing: false };

test("a slash opens the commands only at the start of the message", () => {
  assert.deepEqual(end("/"), { kind: "command", start: 0, query: "" });
  assert.deepEqual(end("/rev"), { kind: "command", start: 0, query: "rev" });
  assert.deepEqual(end("/skill:plan"), { kind: "command", start: 0, query: "skill:plan" });
  assert.equal(end("/review "), null, "a space ends the name");
  assert.equal(end("fix /rev"), null, "not mid-sentence");
  assert.equal(end(" /rev"), null, "not after a space");
  assert.equal(end("/usr/bin"), null, "a path is not a command");
  assert.equal(end("/usr/"), null);
  assert.equal(end("line\n/rev"), null, "only the message's start");
});

test("the caret, not the end of the text, says what is typed", () => {
  assert.deepEqual(triggerAt("/rev and more", 4), { kind: "command", start: 0, query: "rev" });
  assert.equal(triggerAt("/rev and more", 9), null);
});

test("an at sign opens the files at the start or after whitespace", () => {
  assert.deepEqual(end("@"), { kind: "file", start: 0, query: "" });
  assert.deepEqual(end("look at @src/lo"), { kind: "file", start: 8, query: "src/lo" });
  assert.deepEqual(end("a\n@x"), { kind: "file", start: 2, query: "x" });
  assert.deepEqual(end("/review @ma"), { kind: "file", start: 8, query: "ma" });
  assert.equal(end("mail me@example"), null, "an address is not a mention");
  assert.equal(end("@src/a.ts and"), null, "the word ended");
  assert.equal(end('@"my file'), null, "a quoted mention is typed by hand");
});

const commands: AgentCommand[] = [
  { name: "compact", description: "Free up context", hint: "" },
  { name: "review", description: "Review the pending changes", hint: "[branch]" },
  { name: "skill:plan", description: "Plan the work, then review it", hint: "" },
  { name: "$imagegen", description: "Make an image", hint: "" },
  { name: "init", description: "Write a project file", hint: "" },
];

test("commands match by name first, then by description", () => {
  assert.deepEqual(filterCommands(commands, "").map(r => r.label), ["/compact", "/review", "/skill:plan", "$imagegen", "/init"]);
  // `review` starts a name; `skill:plan` only mentions it in its description.
  assert.deepEqual(filterCommands(commands, "rev").map(r => r.label), ["/review", "/skill:plan"]);
  // A name that holds the text comes before a description that does.
  assert.deepEqual(filterCommands(commands, "plan").map(r => r.label), ["/skill:plan"]);
  assert.deepEqual(filterCommands(commands, "IMAGE").map(r => r.label), ["$imagegen"], "case does not matter and a sigil is not typed");
  assert.deepEqual(filterCommands(commands, "zzz"), []);
});

test("a command row carries its description and hint, and what a pick writes", () => {
  const [row] = filterCommands(commands, "review");
  assert.deepEqual(row, { insert: "/review", label: "/review", detail: "Review the pending changes", hint: "[branch]" });
  assert.equal(commandText("$imagegen"), "$imagegen");
  assert.equal(commandText("compact"), "/compact");
});

test("the commands shown are capped", () => {
  const many = Array.from({ length: 100 }, (_, i) => ({ name: `c${i}`, description: "", hint: "" }));
  assert.equal(filterCommands(many, "").length, 40);
  assert.equal(filterCommands(many, "", 5).length, 5);
});

const index = indexFiles(["README.md", "src/main.rs", "src/auth/login.rs", "web/src/login_form.ts", "docs/log.md", "src/logging.rs", "Cargo.toml"]);

test("files match by subsequence of the path, best first", () => {
  assert.deepEqual(filterFiles(index, "log").map(r => r.label), ["log.md", "logging.rs", "login.rs", "login_form.ts"]);
  assert.deepEqual(filterFiles(index, "smn").map(r => r.label), ["main.rs"], "letters in order across the path");
  assert.deepEqual(filterFiles(index, "xyz"), []);
  assert.deepEqual(filterFiles(index, "auth/log").map(r => r.label), ["login.rs"]);
});

test("files nearest the top come first when nothing is typed, and the list is capped", () => {
  assert.deepEqual(filterFiles(index, "").slice(0, 2).map(r => r.label), ["README.md", "Cargo.toml"]);
  assert.equal(filterFiles(index, "", p => p, 3).length, 3);
});

test("a file row names its folder apart and what a pick writes", () => {
  const [row] = filterFiles(index, "login.rs");
  assert.deepEqual(row, { insert: "@src/auth/login.rs", label: "login.rs", detail: "src/auth/", hint: "" });
  const [top] = filterFiles(index, "readme");
  assert.equal(top.detail, "");
  assert.equal(filterFiles(index, "main.rs", p => `/srv/other/${p}`)[0].insert, "@/srv/other/src/main.rs", "another worktree is named in full");
  assert.equal(filterFiles(indexFiles(["my notes.md"]), "notes")[0].insert, '@"my notes.md"', "a space is quoted as a mention is");
});

test("a tree's listings flatten to the files, folders and ignored entries left out", () => {
  const listings = new Map([
    ["", { entries: [{ name: "a.ts", folder: false, ignored: false }, { name: "src", folder: true, ignored: false }, { name: "target", folder: true, ignored: true }] }],
    ["src", { entries: [{ name: "b.ts", folder: false, ignored: false }, { name: "out.log", folder: false, ignored: true }] }],
    ["src/empty", { entries: null }],
  ]);
  assert.deepEqual(filePaths(listings), ["a.ts", "src/b.ts"]);
});

test("the menu's keys: arrows move round the ends, Enter and Tab pick, Esc closes", () => {
  assert.deepEqual(menuKey(3, 0, "ArrowDown", keys), { type: "move", index: 1 });
  assert.deepEqual(menuKey(3, 2, "ArrowDown", keys), { type: "move", index: 0 });
  assert.deepEqual(menuKey(3, 0, "ArrowUp", keys), { type: "move", index: 2 });
  assert.deepEqual(menuKey(3, 1, "Enter", keys), { type: "accept" });
  assert.deepEqual(menuKey(3, 1, "Tab", keys), { type: "accept" });
  assert.deepEqual(menuKey(3, 1, "Escape", keys), { type: "close" });
  assert.deepEqual(menuKey(3, 1, "a", keys), { type: "none" });
});

test("the menu leaves keys alone when it is empty, with a modifier, or while an input method composes", () => {
  assert.deepEqual(menuKey(0, 0, "Enter", keys), { type: "none" }, "Enter sends when no menu shows");
  for (const mod of ["shift", "alt", "ctrl", "meta"] as const) {
    assert.deepEqual(menuKey(3, 0, "Enter", { ...keys, [mod]: true }), { type: "none" }, mod);
  }
  assert.deepEqual(menuKey(3, 0, "Enter", { ...keys, composing: true }), { type: "none" });
  assert.deepEqual(menuKey(3, 0, "ArrowDown", { ...keys, composing: true }), { type: "none" });
});

test("a pick replaces what was typed after the trigger and leaves the rest", () => {
  const command = { insert: "/review", label: "/review", detail: "", hint: "" };
  assert.deepEqual(picked("/rev", 4, { kind: "command", start: 0, query: "rev" }, command), { text: "/review ", caret: 8 });
  const file = { insert: "@src/a.ts", label: "a.ts", detail: "src/", hint: "" };
  const text = "look at @sr please";
  assert.deepEqual(picked(text, 11, { kind: "file", start: 8, query: "sr" }, file), { text: "look at @src/a.ts please", caret: 18 }, "no second space");
  assert.deepEqual(picked("see @s", 6, { kind: "file", start: 4, query: "s" }, file), { text: "see @src/a.ts ", caret: 14 });
});
