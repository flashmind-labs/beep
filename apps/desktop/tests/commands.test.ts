import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap(e => {
    const path = join(dir, e.name);
    return e.isDirectory() ? sources(path) : /\.tsx?$/.test(path) ? [path] : [];
  });
}

/// The registered command names: the last path segment of each entry in
/// `generate_handler![ ... ]`.
function registered() {
  const main = readFileSync("src-tauri/src/lib.rs", "utf8");
  // Desktop-only commands carry a `#[cfg(...)]` attribute, whose bracket is not the macro's end.
  const body = main.slice(main.indexOf("generate_handler![") + "generate_handler![".length).replace(/#\[[^\]]*\]/g, "");
  return body.slice(0, body.indexOf("]")).split(",").map(s => s.trim().split("::").pop()!).filter(Boolean);
}

/// Every `invoke(...)` in src with a literal command name.
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

test("every invoked command is registered in generate_handler", () => {
  const reg = new Set(registered());
  const unknown = calls().filter(c => c.name && !reg.has(c.name)).map(c => `${c.file}:${c.line} ${c.name}`);
  assert.deepEqual(unknown, []);
});
