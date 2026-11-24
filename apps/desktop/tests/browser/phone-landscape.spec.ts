import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  machine_id: "build-box", machine_name: "build-box", id: "s", agent: "codex",
  title: "Retry the sync upload", cwd: "/srv/project", status: "working", external: false,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const view = {
  session, model: null, plan: [], earlier: 0, error: null, turns: 2,
  entries: [...Array.from({ length: 11 }, (_, i) => ({ kind: i % 2 ? "assistant" : "user", text: `Message ${i}` })), { kind: "agent", text: "Read `src/upload.rs` first.", html: "<p>Read <code>src/upload.rs</code> first.</p>" }],
  queued: 0, queued_messages: [],
};

// A touch screen this small takes the phone layout, as a phone does, without the pairing screen.
test.use({ hasTouch: true, isMobile: true });

const home = {
  machines: [], running: [], coming_up: null, updates: null,
  projects: [{ name: "example/app", url: "https://example.test/example/app", machine_id: "build-box", machine_name: "build-box", folder: "/srv/project", cloned: true }],
  needs_you: [],
};

async function openLandscape(page: Page) {
  await page.setViewportSize({ width: 844, height: 390 });
  await openDesktop(page, false, { agent_view: view, center_home: home });
  await expect(page.locator("body")).toHaveClass(/mobile/);
}

test("a phone in landscape has the tabs as a rail on the left", async ({ page }) => {
  await openLandscape(page);
  const tabs = await page.locator("#mb-tabs").boundingBox();
  const main = await page.locator("#main").boundingBox();
  const top = await page.locator(".cc-top").boundingBox();
  expect(tabs && main && top).toBeTruthy();
  if (!tabs || !main || !top) return;

  expect(tabs.x).toBe(0);
  expect(tabs.height).toBe(390);
  expect(main.x).toBeGreaterThanOrEqual(tabs.width);
  expect(main.height).toBe(390);
  expect(top.height).toBeLessThanOrEqual(44);
  await page.screenshot({ path: "../../.artifacts/phone-landscape-home.png" });
});

test("a session in landscape keeps its head to one line", async ({ page }) => {
  await openLandscape(page);
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  const head = page.locator(".dhead").first();
  await expect(head).toBeVisible();
  // The page slides in; it ends flush with the window.
  await expect.poll(async () => (await head.boundingBox())?.x).toBe(0);
  expect((await head.boundingBox())?.height ?? 0).toBeLessThanOrEqual(44);
  // Files and Changes are tabs beside Log, not a pane docked on the right.
  await expect(page.locator(".agshell")).toHaveClass(/narrow/);
  await expect(page.locator("#agmnav")).toBeVisible();
  await page.locator("#agmnav").getByRole("button", { name: "Files" }).click();
  await expect(page.locator(".agconv")).toBeHidden();
  const pane = await page.locator(".agpane:visible").first().boundingBox();
  expect(pane?.x).toBe(0);
  expect(pane?.width).toBe(844);
  await page.screenshot({ path: "../../.artifacts/phone-landscape-files.png" });
  await page.locator("#agmnav").getByRole("button", { name: "Chat" }).click();
  await page.screenshot({ path: "../../.artifacts/phone-landscape-session.png" });
});

test("a file opened from the log takes a phone's screen", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page, false, { agent_view: view, center_home: home });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator("#agmnav [aria-selected=true]")).toHaveText("Chat");
  await page.locator("code.fref", { hasText: "src/upload.rs" }).click();
  await expect(page.locator(".agconv")).toBeHidden();
  await expect(page.locator("#agmnav [aria-selected=true]")).toHaveText("upload.rs");
});

test("a phone reopens a session on its chat, not the last picture it showed", async ({ page }) => {
  // The session had two pictures open, as ones tapped in its chat leave it.
  const pics = ["iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=", "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="].map(data => `data:image/png;base64,${data}`);
  await page.addInitScript(([first, second]) => localStorage.setItem("beep.projectView.build-box\ns", JSON.stringify({
    on: { right: second, bottom: second },
    tabs: [{ path: null, url: first, name: "first.png" }, { path: null, url: second, name: "second.png" }],
  })), pics);
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page, false, { agent_view: view, center_home: home });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator("#agmnav")).toContainText("second.png");
  await expect(page.locator("#agmnav [aria-selected=true]")).toHaveText("Chat");
  await expect(page.locator(".agconv")).toBeVisible();
  // A tab the user picks still takes the screen.
  await page.locator("#agmnav").getByRole("button", { name: "second.png" }).click();
  await expect(page.locator(".agconv")).toBeHidden();
});
