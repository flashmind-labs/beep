import * as esbuild from "esbuild";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const examples = fileURLToPath(new URL("../../plugins/examples/", import.meta.url));

// Plugins run JavaScript; TypeScript compilation belongs in the authoring tools.
export async function buildPlugins(folders = [], watch = false) {
  const sources = folders.length > 0
    ? folders.map((folder) => resolve(folder))
    : readdirSync(examples, { withFileTypes: true })
      .filter((entry) => entry.isDirectory())
      .map((entry) => resolve(examples, entry.name));

  for (const folder of sources) {
    if (!existsSync(resolve(folder, "main.ts"))) continue;

    const manifest = JSON.parse(readFileSync(resolve(folder, "beep-plugin.json"), "utf8"));
    if (manifest === null || typeof manifest !== "object" || Array.isArray(manifest)) {
      throw new Error("A plugin manifest must be an object");
    }
    if ("entry" in manifest && manifest.entry !== "main.js") {
      throw new Error("Set the plugin entry to main.js before building main.ts");
    }

    const options = {
      absWorkingDir: folder,
      entryPoints: ["main.ts"],
      outfile: "main.js",
      bundle: true,
      format: "esm",
      platform: "neutral",
      target: "es2022",
      legalComments: "inline",
      banner: { js: "// Generated from main.ts with npm run plugins:build." },
      logLevel: "warning",
    };

    if (watch) {
      const context = await esbuild.context(options);
      await context.watch();
    } else {
      await esbuild.build(options);
    }
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const args = process.argv.slice(2);
  await buildPlugins(args.filter((arg) => arg !== "--watch"), args.includes("--watch"));
}
