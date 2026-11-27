import test from "node:test";
import assert from "node:assert/strict";
import { composeMessage, fenceAfterBlankLine, fenceSpans, imagePath, pillAt, pillLabel, pillsIn, Pastes, quoteAppend, snapCaret, snapSelection } from "../src/stores/chat/compose-text.ts";

// The fence cases are flsh's own (flashmind-tui repl/input.rs), same
// inputs and outputs.

test("submit fences text after three newlines", () => {
  for (const [input, expected] of [
    ["Explain this\n\n\n    first\n\n    second", "Explain this\n\n\n```\n    first\n\n    second\n```"],
    ["\n\n\n    text", "```\n    text\n```"],
    ["Explain this\n\nMore context\n\n\n    text", "Explain this\n\nMore context\n\n\n```\n    text\n```"],
  ]) {
    assert.equal(fenceAfterBlankLine(input), expected);
    assert.equal(fenceAfterBlankLine(expected), expected);
  }
});

test("submit closes automatic fence at three newlines", () => {
  for (const [input, expected] of [
    [
      "Investigate this:\n\n\nidk idk idk\n...\n\n\nThe above is an error about XYZ",
      "Investigate this:\n\n\n```\nidk idk idk\n...\n```\n\n\nThe above is an error about XYZ",
    ],
    [
      "Explain this\n\n\n    first\n\n    second\n\n\nMore context\n\nAnother paragraph",
      "Explain this\n\n\n```\n    first\n\n    second\n```\n\n\nMore context\n\nAnother paragraph",
    ],
    [
      "hello\n\n\nthis is an example\n\n\nand btw\n\n\n\nthis is also an example\n\n\n",
      "hello\n\n\n```\nthis is an example\n```\n\n\nand btw\n\n\n```\nthis is also an example\n```",
    ],
    ["\n\n\n    text\n\n\nMore context", "```\n    text\n```\n\n\nMore context"],
    ["Explain this\n\n\n    text\n\n\n", "Explain this\n\n\n```\n    text\n```"],
  ]) {
    assert.equal(fenceAfterBlankLine(input), expected);
    assert.equal(fenceAfterBlankLine(expected), expected);
  }
});

test("submit leaves plain, fenced and command input unchanged", () => {
  for (const input of [
    "plain text",
    "first\nsecond",
    "Explain this\n\n    first\n\n    second",
    "Explain this\n\n",
    "Explain this\n\n\n",
    "Explain this\n\n```rust\n    code\n```",
    "Explain this\n\n~~~\n    code\n~~~",
    "Explain this\n\n\n```rust\n    code\n```",
    "Explain this\n\n\n~~~\n    code\n~~~",
    "```\nfirst\n\nsecond\n```",
    "!echo first\n\necho second",
    "/command first\n\nsecond",
    "!echo first\n\n\necho second",
    "/command first\n\n\nsecond",
  ]) {
    assert.equal(fenceAfterBlankLine(input), input.trim());
  }
});

test("the composer outlines every fenced part", () => {
  const value = "a\n\n\nb\n\n\nc\n\n\nd";
  assert.deepEqual(fenceSpans(value, new Pastes()).map(s => value.slice(s.start, s.end)), ["b", "d"]);
  assert.deepEqual(fenceSpans("plain", new Pastes()), []);
});

// ----- Pills -----

test("pills expand before the fence rules look at the text", () => {
  const pastes = new Pastes();
  // A paste that brings its own fence leaves the message unfenced.
  const fenced = pastes.add("```\nfn main() {}\n```\n" + "x\n".repeat(10));
  assert.equal(composeMessage(`Explain this\n\n\n${fenced}`, pastes), `Explain this\n\n\n\`\`\`\nfn main() {}\n\`\`\`\n${"x\n".repeat(9)}x`);
  // A paste's own triple line break closes the fence, as typed text would.
  const two = pastes.add("first\n\n\nsecond\n" + "y\n".repeat(10));
  assert.equal(composeMessage(`Look\n\n\n${two}`, pastes), `Look\n\n\n\`\`\`\nfirst\n\`\`\`\n\n\nsecond\n${"y\n".repeat(9)}y`);
});

test("the caret skips over a pill and deletes it whole", () => {
  const label = pillLabel(1, 12), value = `ab${label}cd`, pills = pillsIn(value);
  const start = 2, end = 2 + label.length;
  assert.equal(snapCaret(pills, start + 1, start), end);
  assert.equal(snapCaret(pills, end - 1, end), start);
  assert.equal(snapCaret(pills, 1, 0), 1);
  assert.deepEqual(snapSelection(pills, 1, start + 3), { start: 1, end });
  assert.deepEqual(snapSelection(pills, start + 2, end + 1), { start, end: end + 1 });
  assert.equal(pillAt(pills, end, true)?.id, 1);
  assert.equal(pillAt(pills, start, false)?.id, 1);
  assert.equal(pillAt(pills, start, true), null);
});

// ----- Picture paths -----

test("a pasted path to a picture is found", () => {
  assert.deepEqual(imagePath("/tmp/shot.png"), { path: "/tmp/shot.png", mime: "image/png" });
  assert.deepEqual(imagePath("  '/tmp/a b.JPG'\n"), { path: "/tmp/a b.JPG", mime: "image/jpeg" });
  assert.deepEqual(imagePath('"/tmp/a.webp"'), { path: "/tmp/a.webp", mime: "image/webp" });
  assert.deepEqual(imagePath("/tmp/My\\ Shot.gif"), { path: "/tmp/My Shot.gif", mime: "image/gif" });
  assert.deepEqual(imagePath("file:///tmp/My%20Shot.png"), { path: "/tmp/My Shot.png", mime: "image/png" });
  assert.deepEqual(imagePath("~/Desktop/shot.jpeg"), { path: "~/Desktop/shot.jpeg", mime: "image/jpeg" });
  assert.equal(imagePath("shot.png"), null);
  assert.equal(imagePath("/tmp/notes.txt"), null);
  assert.equal(imagePath("/tmp/a.png\n/tmp/b.png"), null);
  assert.equal(imagePath("look at /tmp/a.png"), null);
});

test("quoteAppend formats selections as blockquotes without triggering triple-newline fences", () => {
  assert.equal(quoteAppend("existing", "   "), null);
  assert.deepEqual(quoteAppend("", "  keep the query string. \n"), { from: 0, text: "> keep the query string.\n\n" });
  assert.deepEqual(quoteAppend("   \n", "first"), { from: 0, text: "> first\n\n" });

  // Quoting a second selection right after the first replaces trailing newlines so no \n\n\n forms.
  const first = quoteAppend("", "first line");
  assert.ok(first);
  const second = quoteAppend(first.text, "second line");
  assert.ok(second);
  const joined = first.text.slice(0, second.from) + second.text;
  assert.equal(joined, "> first line\n\n> second line\n\n");
  assert.equal(fenceAfterBlankLine(`${joined}Why does this happen?`), "> first line\n\n> second line\n\nWhy does this happen?");

  // Multiple blank lines inside a selection become a single quoted blank line.
  const multi = quoteAppend("Question:", "para one\n\n\npara two");
  assert.ok(multi);
  assert.equal("Question:".slice(0, multi.from) + multi.text, "Question:\n\n> para one\n>\n> para two\n\n");
});
