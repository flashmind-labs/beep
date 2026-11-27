import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { attachmentPictures, promptAttachments } from "../src/stores/agents/promptparts-state.ts";
import { AgentPrompt } from "../src/parts/session/reply-view.tsx";

const shown = (html: string, files: string[]) => renderToStaticMarkup(createElement(AgentPrompt, { prompt: { kind: "prompt", row: "1", html, files, pictures: "" } }));

test("sent attachment paragraphs fold filenames while preserving full paths", () => {
  const paths = Array.from({ length: 28 }, (_, i) => `/path/to/uploads/batch-${i}/files/photo ${i}.jpg`);
  const { html, files } = promptAttachments(`<p><strong>Create models.</strong></p><p>${paths.map(path => `@&quot;${path}&quot;`).join(" ")}</p>`);
  assert.equal(html, "<p><strong>Create models.</strong></p>");
  assert.deepEqual(files, paths);

  const result = document.createElement("div");
  result.innerHTML = shown(html, files);
  assert.equal(result.querySelector("summary")?.textContent, "28 attachments");
  assert.equal(result.querySelector("details")?.hasAttribute("open"), false);
  const chips = [...result.querySelectorAll<HTMLElement>(".attachment-file")];
  assert.equal(chips.length, 28);
  assert.deepEqual(chips.map(chip => chip.getAttribute("title")), paths);
  assert.equal(chips[0].textContent, "photo 0.jpg");
  assert.equal(result.querySelectorAll("button.attachment-file").length, 28);
  assert.equal(chips[0].getAttribute("data-attachment-path"), paths[0]);
});

test("prose, pasted content and code keep literal references; small batches need no disclosure", () => {
  const source = '<p>Look at @src/main.ts first.</p><pre><code>@src/main.ts</code></pre><details><p>@src/keep.ts</p></details>';
  assert.deepEqual(promptAttachments(source), { html: source, files: [] });

  const { html, files } = promptAttachments('<p>@src/main.ts @"/path/to/a &lt;img&gt;.png"</p>');
  assert.deepEqual({ html, files }, { html: "", files: ["src/main.ts", "/path/to/a <img>.png"] });
  const node = document.createElement("div");
  node.innerHTML = shown(html, files);
  assert.equal(node.querySelector("details"), null);
  assert.equal(node.querySelector("img"), null);
  assert.deepEqual([...node.querySelectorAll(".attachment-file")].map(chip => chip.textContent), ["main.ts", "a <img>.png"]);
});

test("original image references pair in order, keeping ambiguous references accessible", () => {
  const files = ["/srv/project/first shot.png", "src/main.ts", "/srv/project/second.JPG"];
  assert.deepEqual(attachmentPictures(files, 2), [files[0], files[2]]);
  assert.deepEqual(attachmentPictures(files, 1), []);
  assert.deepEqual(attachmentPictures(files, 0), []);
});
