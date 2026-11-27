import { build } from "esbuild";
import { expect, test } from "@playwright/test";

// `openScreen` only signals: the bar's bundle must carry the follower that opens what it asks for.
test("the bar bundle follows the screen requests its views make", async () => {
  const built = await build({
    entryPoints: ["src/bar/app.ts"], bundle: true, write: false, metafile: true, outdir: "out", splitting: true,
    format: "esm", loader: { ".svg": "text" }, logLevel: "silent",
  });
  const inputs = Object.keys(built.metafile.inputs);

  expect(inputs).toContain("src/center/nav.ts");
  expect(inputs).toContain("src/center/screens.ts");
  // Settings, and with it the instructions and providers pages, open from the bar through the request.
  expect(inputs).toContain("src/settings/sheet.ts");
  expect(inputs).toContain("src/settings/settings.tsx");
  expect(inputs).toContain("src/settings/instructions.tsx");
  expect(inputs).toContain("src/settings/providers.tsx");
  expect(inputs).not.toContain("src/center/center.ts");
});
