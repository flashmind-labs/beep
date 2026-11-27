import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

test("sandboxed diagrams load from the Tauri origin in WebKit", { skip: process.platform !== "darwin", timeout: 45000 }, async () => {
  const desktop = new URL("../../desktop/", import.meta.url);
  const config = JSON.parse(await readFile(new URL("src-tauri/tauri.conf.json", desktop), "utf8"));
  const bundle = await build({
    entryPoints: [fileURLToPath(new URL("src/parts/session/diagrams.tsx", desktop))], bundle: true, format: "iife", write: false,
    plugins: [{ name: "native-boundary", setup(builder) {
      builder.onLoad({ filter: /\/parts\/session\/media\.tsx$/ }, () => ({ contents: "export const notice=()=>{};export const saveUrl=async()=>{};" }));
      builder.onLoad({ filter: /\/ui\/tauri\.ts$/ }, () => ({ contents: "export const invoke=async()=>null;" }));
    } }],
  });
  const script = fileURLToPath(new URL("native-webkit.swift", import.meta.url));
  const assets = fileURLToPath(new URL("../assets/", import.meta.url));
  const child = spawn("swift", [script, assets, config.app.security.csp], { stdio: ["pipe", "pipe", "pipe"] });
  let output = "";
  child.stdout.on("data", chunk => { output += chunk; });
  child.stderr.on("data", chunk => { output += chunk; });
  const exit = new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (code, signal) => resolve({ code, signal }));
  });
  child.stdin.end(bundle.outputFiles[0].text);
  assert.deepEqual(await exit, { code: 0, signal: null }, output);
  assert.match(output, /rendered/);
});
