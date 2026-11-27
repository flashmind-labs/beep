// Bundles src/ into dist/, next to the HTML and CSS Tauri serves.
// `--watch` rebuilds on every change, for `cargo tauri dev`.
import * as esbuild from "esbuild";
import { buildPlugins } from "./build-plugins.mjs";
import { copyFileSync, cpSync, mkdirSync, readdirSync, rmSync } from "node:fs";

await buildPlugins([], process.argv.includes("--watch"));

cpSync(new URL("../diagrams/assets/", import.meta.url), new URL("dist/diagrams/", import.meta.url), { recursive: true });

// Ghostty's terminal engine, loaded by terminals/termview.ts next to the bundle.
copyFileSync(
  new URL("ghostty/ghostty-vt.wasm", import.meta.url),
  new URL("dist/ghostty-vt.wasm", import.meta.url),
);

// Chunks from earlier builds, named by their hash, would ship with the app.
const dist = new URL("dist/", import.meta.url);
for (const f of readdirSync(dist)) if (/-[A-Z0-9]{8}\.js$/.test(f)) rmSync(new URL(f, dist));

const pdf = new URL("node_modules/pdfjs-dist/", import.meta.url);
const pdfAssets = new URL("pdfjs/", dist);
mkdirSync(pdfAssets, { recursive: true });
copyFileSync(new URL("legacy/build/pdf.worker.min.mjs", pdf), new URL("pdf-worker.js", pdfAssets));
for (const dir of ["cmaps", "iccs", "standard_fonts", "wasm"]) {
  cpSync(new URL(dir, pdf), new URL(dir, pdfAssets), { recursive: true });
}

const options = {
  absWorkingDir: import.meta.dirname,
  // One bundle per window, named for the HTML page that loads it.
  entryPoints: {
    app: "src/shell/bar/app.ts",
    center: "src/shell/center/center.ts",
  },
  outdir: "dist",
  // Painted scenes kept as SVG files are bundled as their text.
  loader: { ".svg": "text" },
  bundle: true,
  format: "esm",
  target: "safari16",
  // three.js, xterm.js and other modules a window needs only on use load in their own chunks.
  splitting: true,
  logLevel: "warning",
  // Minified outside watch mode: less to load and parse when a window opens. Functions keep
  // their names, for stack traces in logs.
  minify: !process.argv.includes("--watch"),
  keepNames: true,
  // React's development build validates and times every render; ship the production one.
  define: { "process.env.NODE_ENV": process.argv.includes("--watch") ? '"development"' : '"production"' },
};

if (process.argv.includes("--watch")) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
} else {
  await esbuild.build(options);
}
