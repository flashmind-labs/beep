import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { build } from "esbuild";
import { chromium, webkit } from "playwright";

const bundle = await build({ entryPoints: [new URL("../../desktop/src/chat/diagrams.ts", import.meta.url).pathname], bundle: true, format: "esm", write: false });
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
    if (path === "/") return route.fulfill({ contentType: "text/html", body: `<!doctype html><html><body>
      <div id="bar"><div id="reply"><pre><code class="language-mermaid-pending">flowchart LR\n A--&gt;B</code></pre></div></div>
      <script>window.calls=[];window.__TAURI__={core:{invoke:async(name,args)=>{calls.push({name,args});return name==='save_picture'?'/downloads/'+args.name:null},Channel:class{}},event:{listen:async()=>()=>{}}};</script>
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
  await preview.locator("#svg:not(:disabled)").waitFor();
  assert.equal(loads, 1);
  // Conversation panes detach their DOM while another session is open.
  await page.evaluate(async () => {
    const reply = document.getElementById("reply");
    reply.innerHTML = `<pre><code class="language-mermaid">sequenceDiagram\n A-&gt;&gt;B: Hello</code></pre>`;
    reply.remove();
    await new Promise(requestAnimationFrame);
    document.getElementById("bar").append(reply);
  });
  await preview.locator("#svg:not(:disabled)").waitFor({ timeout: 5000 });
  await page.evaluate(async () => {
    const reply = document.getElementById("reply");
    reply.remove();
    await new Promise(requestAnimationFrame);
    document.getElementById("bar").append(reply);
  });
  await preview.locator("#svg:not(:disabled)").waitFor({ timeout: 5000 });
  assert.equal(await page.locator('iframe[title="Mermaid diagram"]').getAttribute("sandbox"), "allow-scripts");
  await preview.locator("#svg").click();
  await page.waitForFunction(() => calls.some(c => c.name === "save_picture"));
  const saved = await page.evaluate(() => calls.find(c => c.name === "save_picture").args);
  assert.equal(saved.name, "diagram.svg");
  assert.match(saved.url, /^data:image\/svg\+xml;base64,/);
  await preview.locator("#copy-image").click();
  await page.waitForFunction(() => calls.some(c => c.name === "copy_diagram_image"));
  const copied = await page.evaluate(() => calls.find(c => c.name === "copy_diagram_image").args);
  assert.equal(Buffer.from(copied.png, "base64").subarray(1, 4).toString(), "PNG");
  await preview.locator("#picture").click({ button: "right" });
  await preview.getByRole("menuitem", { name: "Save image to Downloads", exact: true }).click();
  await page.waitForFunction(() => calls.some(c => c.args?.name === "diagram.png"));
  await preview.locator("#expand").click();
  const expanded = page.frameLocator("dialog iframe");
  await expanded.locator("#png:not(:disabled)").waitFor();
  const width = await expanded.locator("#picture").evaluate(img => img.clientWidth);
  await expanded.getByRole("button", { name: "Zoom in" }).click();
  assert.ok(await expanded.locator("#picture").evaluate(img => img.clientWidth) > width);
  await expanded.locator("#picture").focus();
  await page.keyboard.press("Meta+c");
  await page.waitForFunction(() => calls.filter(c => c.name === "copy_diagram_image").length === 2);
  await expanded.locator("#picture").click({ button: "right" });
  await expanded.getByRole("menuitem", { name: "Copy image", exact: true }).click();
  await page.waitForFunction(() => calls.filter(c => c.name === "copy_diagram_image").length === 3);
  await expanded.locator("#png").click();
  await page.waitForFunction(() => calls.some(c => c.args?.name === "diagram.png"));
  await expanded.locator("#expand").click();
  await page.locator("dialog").waitFor({ state: "detached" });
  assert.equal(loads, 1);
  assert.deepEqual(errors, []);
});
