import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

/// Commands Rust registers that nothing in src calls yet. Wire one up or
/// delete it from Rust, then remove it here.
const UNCALLED = ["agent_preview", "center_open_project", "center_project_send", "mesh_remove", "open_center"];

/// The one name built at run time: orb.ts calls `${act}_notice`, and `act` is
/// "play", "open" or "dismiss".
const BUILT = ["dismiss_notice", "open_notice", "play_notice"];

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(e => {
    const path = join(dir, e.name);
    return e.isDirectory() ? sources(path) : path.endsWith(".ts") ? [path] : [];
  });
}

/// The registered command names: the last path segment of each entry in
/// `generate_handler![ ... ]`.
function registered() {
  const main = readFileSync("src-tauri/src/main.rs", "utf8");
  const body = main.slice(main.indexOf("generate_handler![") + "generate_handler![".length);
  return body.slice(0, body.indexOf("]")).split(",").map(s => s.trim().split("::").pop()!).filter(Boolean);
}

/// Every `invoke(...)` in src with a literal command name.
/// A call with a variable or ternary name is covered by `quoted` below.
function calls() {
  const found: { file: string; line: number; name: string | null }[] = [];
  for (const file of sources("src")) {
    const text = readFileSync(file, "utf8");
    for (const m of text.matchAll(/\binvoke\b/g)) {
      let i = m.index! + m[0].length;
      if (text[i] === "<") {
        let depth = 0;
        for (; i < text.length; i++) {
          if (text[i] === "<") depth++;
          else if (text[i] === ">" && text[i - 1] !== "=" && --depth === 0) { i++; break; }
        }
      }
      if (text[i] !== "(") continue;
      const name = /^\(\s*"([a-z0-9_]+)"/.exec(text.slice(i, i + 80))?.[1] ?? null;
      found.push({ file, line: text.slice(0, m.index).split("\n").length, name });
    }
  }
  return found;
}

/// Every quoted lowercase word in src. Commands also reach `invoke` through
/// helpers (`loadList("list_skills")`, a `cmd` parameter), so a registered name
/// that appears quoted anywhere counts as called.
function quoted() {
  return new Set(sources("src").flatMap(f => [...readFileSync(f, "utf8").matchAll(/["']([a-z][a-z0-9_]*)["']/g)].map(m => m[1])));
}

test("every invoked command is registered in generate_handler", () => {
  const reg = new Set(registered());
  const unknown = calls().filter(c => c.name && !reg.has(c.name)).map(c => `${c.file}:${c.line} ${c.name}`);
  assert.deepEqual(unknown, []);
});

test("every registered command is called, or listed in UNCALLED", () => {
  const used = quoted();
  const idle = registered().filter(n => !used.has(n) && !BUILT.includes(n));
  assert.deepEqual(idle.filter(n => !UNCALLED.includes(n)), [], "registered but never called");
  assert.deepEqual(UNCALLED.filter(n => !idle.includes(n)), [], "listed in UNCALLED but now called or gone");
});

test("every name in BUILT is a registered command", () => {
  const reg = new Set(registered());
  assert.deepEqual(BUILT.filter(n => !reg.has(n)), []);
});
