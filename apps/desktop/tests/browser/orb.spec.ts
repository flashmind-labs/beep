import { readFile } from "node:fs/promises";
import { extname, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

test("orb keeps cards mounted while live events arrive", async ({ page }) => {
  const dist = fileURLToPath(new URL("../../dist/", import.meta.url));
  await page.route("https://app.test/**", async route => {
    const path = resolve(dist, new URL(route.request().url()).pathname.replace(/^\/+/, ""));
    if (relative(dist, path).startsWith("..")) return route.fulfill({ status: 403 });
    try { await route.fulfill({ body: await readFile(path), contentType: extname(path) === ".html" ? "text/html" : extname(path) === ".js" ? "text/javascript" : "text/css" }); }
    catch { await route.fulfill({ status: 404 }); }
  });
  await page.addInitScript(() => {
    const handlers = new Map<string, (event: { payload: unknown }) => void>();
    Object.assign(window, { __orbEvent: (name: string, payload: unknown) => handlers.get(name)?.({ payload }), __TAURI__: {
      core: { invoke: async (command: string) => command === "live_approval" || command === "list_apps" ? [] : null, Channel: class {} },
      event: { listen: async (name: string, handler: (event: { payload: unknown }) => void) => { handlers.set(name, handler); return () => handlers.delete(name); } },
    } });
  });
  await page.goto("https://app.test/orb.html");
  await expect(page.locator("#orb")).toBeVisible();
  await page.evaluate(() => {
    const emit = (window as unknown as { __orbEvent: (name: string, payload: unknown) => void }).__orbEvent;
    emit("live", { kind: "approval", session: "sample", work: false, approval: { query_id: "one", app: "Example", action: "Read", fields: [{ label: "File", value: "example.txt" }] } });
  });
  const card = page.locator("#ask");
  await expect(card).toContainText("example.txt");
  await card.evaluate(el => { el.dataset.identity = "kept"; });
  await page.evaluate(() => (window as unknown as { __orbEvent: (name: string, payload: unknown) => void }).__orbEvent("live", { kind: "view", phase: "listening", words: "", line: null, speaking: false, title: null, finishing: false }));
  await expect(card).toHaveAttribute("data-identity", "kept");
  await expect(card).toContainText("example.txt");
  await page.evaluate(() => {
    const emit = (window as unknown as { __orbEvent: (name: string, payload: unknown) => void }).__orbEvent;
    emit("live", { kind: "soon", ms: 500 });
    emit("live", { kind: "toast", text: "Listening" });
  });
  await expect(page.locator("#ring")).toBeVisible();
  await page.evaluate(() => {
    const emit = (window as unknown as { __orbEvent: (name: string, payload: unknown) => void }).__orbEvent;
    emit("live", { kind: "view", phase: "thinking", words: "", line: null, speaking: false, title: null, finishing: false });
  });
  await expect(page.locator("#orb")).toHaveClass(/think/);
  await card.getByText("Allow", { exact: true }).click();
  await expect(card).toBeHidden();
  await page.evaluate(() => (window as unknown as { __orbEvent: (name: string, payload: unknown) => void }).__orbEvent("live", { kind: "left" }));
  await expect(page.locator("#orb")).toHaveClass(/think/);
  await expect(page.locator("#toast")).toBeHidden();
  await expect(page.locator("#ring")).toBeHidden();
});
