import { build } from "esbuild";
import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const responses = {
  settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
  list_machines: [], list_credentials: [], list_github_accounts: [],
  instructions_get: { version: 1, text: "Example instructions", note: "", saved_at: 0 },
};

// `openScreen` only signals: the bar's bundle must carry the follower that opens what it asks for.
test("the bar bundle follows the screen requests its views make", async () => {
  const built = await build({
    entryPoints: ["src/shell/bar/app.ts"], bundle: true, write: false, metafile: true, outdir: "out", splitting: true,
    format: "esm", loader: { ".svg": "text" }, logLevel: "silent",
  });
  const inputs = Object.keys(built.metafile.inputs);

  expect(inputs).toContain("src/parts/center/nav.ts");
  expect(inputs).toContain("src/shell/center/screens.ts");
  expect(inputs).not.toContain("src/shell/center/center.ts");
});

test("a settings request made in the bar opens the sheet, and a second one moves it to its section", async ({ page }) => {
  // The bar's own bundle, with `openScreen` of the same module instance handed to the page.
  const built = await build({
    stdin: { contents: 'import "./src/shell/bar/app.ts"; import { openScreen } from "./src/parts/center/nav.ts"; Object.assign(window, { openScreen });', resolveDir: ".", loader: "ts" },
    bundle: true, write: false, format: "esm", loader: { ".svg": "text" }, logLevel: "silent",
  });
  const script = built.outputFiles?.[0]?.text ?? "";
  await openDesktop(page, false, responses);
  await page.route("https://app.test/app.js", route => route.fulfill({ body: script, contentType: "text/javascript" }));
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar > *").first()).toBeVisible();
  await page.waitForFunction(() => "openScreen" in window);
  await page.evaluate(() => Reflect.get(window, "openScreen")({ screen: "settings", section: null }));
  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings).toBeVisible();
  await expect(page.locator("#set-general")).toBeVisible();
  await page.evaluate(() => Reflect.get(window, "openScreen")({ screen: "settings", section: "machines" }));
  await expect(page.locator("#set-general")).toHaveCount(0);
  await expect(settings).toBeVisible();
});
