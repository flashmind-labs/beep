// Bundles src/ into dist/, next to the HTML and CSS Tauri serves.
// `--watch` rebuilds on every change, for `cargo tauri dev`.
import * as esbuild from "esbuild";

const options = {
  absWorkingDir: import.meta.dirname,
  entryPoints: ["src/app.ts", "src/center.ts", "src/orb.ts", "src/research.ts"],
  outdir: "dist",
  bundle: true,
  format: "esm",
  target: "safari16",
  logLevel: "warning",
};

if (process.argv.includes("--watch")) {
  const ctx = await esbuild.context(options);
  await ctx.watch();
} else {
  await esbuild.build(options);
}
