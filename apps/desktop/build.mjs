// Bundles src/ into dist/, next to the HTML and CSS Tauri serves.
// `--watch` rebuilds on every change, for `cargo tauri dev`.
import * as esbuild from "esbuild";
import { copyFileSync, readdirSync, rmSync } from "node:fs";

// xterm.js's stylesheet, linked from center.html next to the bundle.
copyFileSync(
  new URL("node_modules/@xterm/xterm/css/xterm.css", import.meta.url),
  new URL("dist/xterm.css", import.meta.url),
);

// Chunks from earlier builds, named by their hash, would ship with the app.
const dist = new URL("dist/", import.meta.url);
for (const f of readdirSync(dist)) if (/-[A-Z0-9]{8}\.js$/.test(f)) rmSync(new URL(f, dist));

const options = {
  absWorkingDir: import.meta.dirname,
  // One bundle per window, named for the HTML page that loads it.
  entryPoints: {
    app: "src/bar/app.ts",
    center: "src/center/center.ts",
    orb: "src/orb/orb.ts",
  },
  outdir: "dist",
  bundle: true,
  format: "esm",
  target: "safari16",
  // three.js, for the media pane's 3D viewer, loads in its own chunk when first used.
  splitting: true,
  logLevel: "warning",
};

if (process.argv.includes("--watch")) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
} else {
  await esbuild.build(options);
}
