// Modules run in import order, so a cycle can run a page before what it needs.
// A page that calls registerView as it loads crashes the whole window then.
import { test } from "node:test";
import assert from "node:assert/strict";
import * as esbuild from "esbuild";

/// The modules of `entry` in the order they run, following static imports.
async function runOrder(entry: string) {
  const { metafile } = await esbuild.build({ entryPoints: [entry], bundle: true, write: false, metafile: true, format: "esm", outdir: "unused", splitting: true, logLevel: "silent" });
  const seen = new Set<string>(), order: string[] = [];
  const visit = (file: string) => {
    if (seen.has(file)) return;
    seen.add(file);
    for (const dep of metafile.inputs[file]?.imports ?? []) if (dep.kind === "import-statement") visit(dep.path);
    order.push(file);
  };
  visit(entry);
  return order;
}

test("the command center runs its state before any page", async () => {
  const order = await runOrder("src/center/center.ts");
  const state = order.indexOf("src/center/state.ts");
  assert.ok(state >= 0);
  for (const page of ["src/project/page.ts", "src/center/views.ts", "src/center/home.ts"]) {
    assert.ok(order.indexOf(page) > state, `${page} runs before center/state.ts`);
  }
});

test("the bar's window loads no command center page", async () => {
  const order = await runOrder("src/bar/app.ts");
  assert.deepEqual(order.filter(file => file.startsWith("src/center/") || file === "src/project/page.ts"), []);
});
