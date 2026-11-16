import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { build } from "esbuild";
import { chromium, webkit } from "playwright";

// The diagram scanner, and a reply row that `streamReply` renders the way a streamed turn does.
const bundle = await build({ stdin: { contents: `import "./chat/diagrams.tsx";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { AgentReply } from "./agents/reply-view.tsx";
let root = null;
window.streamReply = (host, html) => flushSync(() => (root ??= createRoot(host)).render(createElement(AgentReply, { reply: { kind: "reply", row: "r", markdown: true, html } })));`,
  resolveDir: new URL("../../desktop/src", import.meta.url).pathname, loader: "tsx" }, bundle: true, format: "esm", jsx: "automatic", write: false });
const engine = process.env.DIAGRAM_ENGINE === "webkit" ? webkit : chromium;

test("desktop renders through streamed updates, caches, expands and exports", async t => {
  const browser = await engine.launch({ executablePath: process.env.DIAGRAM_BROWSER });
  t.after(() => browser.close());
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  let loads = 0;
  await page.route("https://diagram.test/**", async route => {
    const path = new URL(route.request().url()).pathname;
    if (path === "/") return route.fulfill({ contentType: "text/html", body: `<!doctype html><html><head><style>pre:not([data-diagram]) > code.language-mermaid { animation: diagram-found 1ms } @keyframes diagram-found { from { opacity: .99 } }</style></head><body>
      <div id="bar"><div id="reply"><pre><code class="language-mermaid-pending">flowchart LR\n A--&gt;B</code></pre></div></div>
      <script>window.calls=[];window.__TAURI__={core:{invoke:async(name,args)=>{calls.push({name,args});return name==='save_picture'?'/downloads/'+args.name:null},Channel:class{}},event:{listen:async()=>()=>{}},webviewWindow:{getCurrentWebviewWindow:()=>new Proxy({},{get:()=>async()=>()=>{}})}};</script>
      <script type="module">await import('/app.js');window.ready=true;</script></body></html>` });
    if (path === "/app.js") return route.fulfill({ contentType: "text/javascript", body: bundle.outputFiles[0].text });
    if (path === "/diagrams/mermaid.min.js") loads++;
    const name = path.split("/").pop();
    if (!["index.html", "renderer.js", "mermaid.min.js"].includes(name)) return route.abort();
    return route.fulfill({ body: await readFile(new URL(`../assets/${name}`, import.meta.url)), contentType: name.endsWith(".html") ? "text/html" : "text/javascript" });
  });
  await page.goto("https://diagram.test/");
  await page.waitForFunction(() => window.ready);
  assert.equal(await page.locator("iframe").count(), 0);
  await page.evaluate(async () => {
    for (let i = 0; i < 24; i++) {
      document.getElementById("reply").innerHTML = `<pre><code class="language-mermaid">flowchart LR\n A--&gt;B</code></pre><p>${i}</p>`;
      await new Promise(requestAnimationFrame);
    }
  });
  const preview = page.frameLocator('iframe[title="Mermaid diagram"]').first();
  await preview.locator("#export:not(:disabled)").waitFor();
  assert.equal(loads, 1);
  // Conversation panes detach their DOM while another session is open.
  await page.evaluate(async () => {
    const reply = document.getElementById("reply");
    reply.innerHTML = `<pre><code class="language-mermaid">sequenceDiagram\n A-&gt;&gt;B: Hello</code></pre>`;
    reply.remove();
    await new Promise(requestAnimationFrame);
    document.getElementById("bar").append(reply);
  });
  await preview.locator("#export:not(:disabled)").waitFor({ timeout: 5000 });
  await page.evaluate(async () => {
    const reply = document.getElementById("reply");
    reply.remove();
    await new Promise(requestAnimationFrame);
    document.getElementById("bar").append(reply);
  });
  await preview.locator("#export:not(:disabled)").waitFor({ timeout: 5000 });
  assert.equal(await page.locator('iframe[title="Mermaid diagram"]').getAttribute("sandbox"), "allow-scripts");
  await preview.locator("#export").click(); await preview.locator("#svg").click();
  await page.waitForFunction(() => calls.some(c => c.name === "save_picture"));
  const saved = await page.evaluate(() => calls.find(c => c.name === "save_picture").args);
  assert.equal(saved.name, "diagram.svg");
  assert.match(saved.url, /^data:image\/svg\+xml;base64,/);
  await preview.locator("#export").click(); await preview.locator("#copy-image").click();
  await page.waitForFunction(() => calls.some(c => c.name === "copy_diagram_image"));
  const copied = await page.evaluate(() => calls.find(c => c.name === "copy_diagram_image").args);
  assert.equal(Buffer.from(copied.png, "base64").subarray(1, 4).toString(), "PNG");
  await preview.locator("#picture").click({ button: "right" });
  await preview.getByRole("menuitem", { name: "Save image to Downloads", exact: true }).click();
  await page.waitForFunction(() => calls.some(c => c.args?.name === "diagram.png"));
  await preview.locator("#expand").click();
  const expanded = page.frameLocator("dialog iframe");
  await expanded.locator("#export:not(:disabled)").waitFor();
  const width = await expanded.locator("#picture").evaluate(img => img.clientWidth);
  await expanded.getByRole("button", { name: "Zoom in" }).click();
  assert.ok(await expanded.locator("#picture").evaluate(img => img.clientWidth) > width);
  await expanded.locator("#picture").focus();
  await page.keyboard.press("Meta+c");
  await page.waitForFunction(() => calls.filter(c => c.name === "copy_diagram_image").length === 2);
  await expanded.locator("#picture").click({ button: "right" });
  await expanded.getByRole("menuitem", { name: "Copy image", exact: true }).click();
  await page.waitForFunction(() => calls.filter(c => c.name === "copy_diagram_image").length === 3);
  await expanded.locator("#export").click(); await expanded.locator("#png").click();
  await page.waitForFunction(() => calls.some(c => c.args?.name === "diagram.png"));
  await expanded.locator("#expand").click();
  await page.locator("dialog").waitFor({ state: "detached" });
  // A streamed reply keeps a finished diagram's frame while the text after it grows.
  await page.evaluate(() => {
    const host = document.createElement("div"); host.id = "streamed"; document.getElementById("bar").append(host);
    streamReply(host, `<p>Intro</p><pre><code class="language-mermaid-pending">flowchart LR\n C--&gt;</code></pre>`);
    streamReply(host, `<p>Intro</p><pre><code class="language-mermaid">flowchart LR\n C--&gt;D</code></pre>`);
  });
  const streamed = page.locator('#streamed iframe[title="Mermaid diagram"]');
  await page.frameLocator('#streamed iframe[title="Mermaid diagram"]').locator("#export:not(:disabled)").waitFor();
  await streamed.evaluate(frame => { frame.dataset.first = "true"; });
  await page.evaluate(async () => {
    for (let i = 0; i < 12; i++) {
      streamReply(document.getElementById("streamed"), `<p>Intro</p><pre><code class="language-mermaid">flowchart LR\n C--&gt;D</code></pre><p>${"word ".repeat(i)}</p>`);
      await new Promise(requestAnimationFrame);
    }
  });
  assert.equal(await streamed.count(), 1);
  assert.equal(await streamed.getAttribute("data-first"), "true");
  assert.equal(loads, 1);
  assert.deepEqual(errors, []);
});
