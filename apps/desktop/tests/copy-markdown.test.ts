import { test } from "node:test";
import { strict as assert } from "node:assert";
import { JSDOM } from "jsdom";

const dom = new JSDOM('<div class="dcoderen agent md"><p>Hello <strong>world</strong></p><table><thead><tr><th>Name</th><th>Value</th></tr></thead><tbody><tr><td>One</td><td>Two</td></tr></tbody></table></div><p>Outside</p>');
Object.assign(globalThis, { document: dom.window.document, window: dom.window, Element: dom.window.Element });
await import("../src/stores/agents/copy-markdown.ts");

test("copies selected output as Markdown", () => {
  const row = document.querySelector(".agent")!;
  const range = document.createRange();
  range.setStart(row.querySelector("p")!.firstChild!, 0);
  range.setEnd(row.querySelector("td:last-child")!.firstChild!, 3);
  const selection = window.getSelection()!;
  selection.removeAllRanges(); selection.addRange(range);
  let copied = "";
  const event = new dom.window.Event("copy", { cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: { setData: (_type: string, value: string) => { copied = value; } } });
  document.dispatchEvent(event);
  assert.match(copied, /Hello \*\*world\*\*/);
  assert.match(copied, /\| Name\s*\| Value\s*\|/);
  assert.match(copied, /\| One\s*\| Two\s*\|/);
  assert.equal(event.defaultPrevented, true);
});


test("quote copying preserves paragraphs, links and nested content without controls", async () => {
  const { quotedMarkdown } = await import("../src/stores/agents/copy-markdown.ts");
  const quote = document.createElement("blockquote");
  quote.innerHTML = '<button>Copied</button><p>First <code>command</code>.</p><p><a href="https://example.com">Details</a></p><blockquote><button>Copy</button><p>Nested quote</p></blockquote><pre><button>Copy</button><code>echo hello</code></pre>';
  const text = quotedMarkdown(quote);
  assert.match(text, /First `command`\.\n\n/);
  assert.match(text, /\[Details\]\(https:\/\/example.com\)/);
  assert.match(text, /> Nested quote/);
  assert.match(text, /echo hello/);
  assert.doesNotMatch(text, /Copied|Copy/);
});
