// One pinned, offline engine and its licenses ship with every client.
import { build } from "esbuild";
import { copyFileSync, readFileSync, readdirSync, writeFileSync } from "node:fs";

const result = await build({
  absWorkingDir: import.meta.dirname,
  entryPoints: ["engine.js"],
  outfile: "assets/mermaid.min.js",
  bundle: true,
  minify: true,
  format: "iife",
  target: ["safari16", "chrome100"],
  legalComments: "eof",
  metafile: true,
});
copyFileSync(new URL("node_modules/mermaid/LICENSE", import.meta.url), new URL("assets/MERMAID-LICENSE", import.meta.url));
const packages = new Set(Object.keys(result.metafile.inputs).flatMap(path => path.match(/^node_modules\/(?:@[^/]+\/)?[^/]+/) ?? []));
const licenses = [...packages].sort().map(path => {
  const folder = new URL(`${path}/`, import.meta.url);
  const info = JSON.parse(readFileSync(new URL("package.json", folder), "utf8"));
  const files = readdirSync(folder).filter(name => /^(licen[sc]e|copying|notice)(\.|$|-)/i.test(name));
  return `${info.name}@${info.version} (${info.license ?? ""})\n\n${files.map(name => readFileSync(new URL(name, folder), "utf8")).join("\n")}`;
});
writeFileSync(new URL("assets/THIRD-PARTY-LICENSES.txt", import.meta.url), licenses.join("\n\n----------------\n\n"));
