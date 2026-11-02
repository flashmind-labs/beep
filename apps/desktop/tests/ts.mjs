// Lets `node --test` load the TypeScript sources: esbuild strips the types,
// so this works on a Node built without its own TypeScript support.
// Run as `node --import ./tests/ts.mjs --test tests/*.test.ts`.
import { registerHooks } from "node:module";
import { readFileSync } from "node:fs";
import { transformSync } from "esbuild";

registerHooks({
  load(url, context, next) {
    if (!url.startsWith("file:") || !url.endsWith(".ts")) return next(url, context);
    const { code } = transformSync(readFileSync(new URL(url), "utf8"), { loader: "ts", format: "esm", sourcefile: url });
    return { format: "module", source: code, shortCircuit: true };
  },
});
