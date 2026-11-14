import test from "node:test";
import assert from "node:assert/strict";
import { startDom } from "./start-react-env.ts";
import { compactPromptAttachments } from "../src/agents/promptparts-state.ts";

test("sent attachment paragraphs fold filenames while preserving full paths", () => {
  const env = startDom();
  try {
    const paths = Array.from({ length: 28 }, (_, i) => `/path/to/uploads/batch-${i}/files/photo ${i}.jpg`);
    const result = env.dom.window.document.createElement("div");
    result.innerHTML = compactPromptAttachments(`<p><strong>Create models.</strong></p><p>${paths.map(path => `@&quot;${path}&quot;`).join(" ")}</p>`);
    assert.equal(result.querySelector("strong")?.textContent, "Create models.");
    assert.equal(result.querySelector("summary")?.textContent, "28 attachments");
    assert.equal(result.querySelector("details")?.open, false);
    const chips = [...result.querySelectorAll<HTMLElement>(".attachment-file")];
    assert.equal(chips.length, 28);
    assert.deepEqual(chips.map(chip => chip.title), paths);
    assert.equal(chips[0].textContent, "photo 0.jpg");
    assert.equal(chips[0].tagName, "BUTTON");
    assert.equal(chips[0].dataset.attachmentPath, paths[0]);
    assert.equal(result.textContent?.includes("/path/to"), false);
  } finally { env.close(); }
});

test("prose, pasted content and code keep literal references; small batches need no disclosure", () => {
  const env = startDom();
  try {
    const source = '<p>Look at @src/main.ts first.</p><pre><code>@src/main.ts</code></pre><details><p>@src/keep.ts</p></details>';
    assert.equal(compactPromptAttachments(source), source);
    const node = env.dom.window.document.createElement("div");
    node.innerHTML = compactPromptAttachments('<p>@src/main.ts @"/path/to/a &lt;img&gt;.png"</p>');
    assert.equal(node.querySelector("details"), null);
    assert.equal(node.querySelectorAll(".attachment-file").length, 2);
    assert.equal(node.querySelector("img"), null);
    assert.equal(node.querySelector(".attachment-file")?.textContent, "main.ts");
  } finally { env.close(); }
});

test("a message that shows its pictures names only its other files", () => {
  const env = startDom();
  try {
    const node = env.dom.window.document.createElement("div");
    node.innerHTML = compactPromptAttachments('<p>See these.</p><p>@/path/to/shot.png @src/main.ts</p><p>@"/path/to/second shot.JPG"</p>', true);
    assert.deepEqual([...node.querySelectorAll(".attachment-file")].map(chip => chip.textContent), ["main.ts"]);
    assert.equal(node.querySelectorAll("p").length, 1);
  } finally { env.close(); }
});
