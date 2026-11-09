import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("a reopened session restores its file tab", async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem("beep.projectView.build-box\nnotice-one", JSON.stringify({
    on: { right: "text:sample.ts", bottom: "text:sample.ts" }, tabs: [{ root: "/srv/project", rel: "sample.ts" }],
  })));
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  }}));
  await expect(page.getByRole("tab", { name: /^sample.ts Close/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /^sample.ts Close/ })).toHaveAttribute("aria-selected", "true");
  const code = page.locator(".agcode:visible");
  await code.evaluate(node => { node.scrollTop = 700; });
  await expect.poll(() => code.evaluate(node => node.scrollTop)).toBe(700);
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await page.evaluate(() => window.__navigation.open({agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  }}));
  await expect.poll(() => page.locator(".agcode:visible").evaluate(node => node.scrollTop)).toBe(700);
  await page.screenshot({ path: "test-results/restored-file-tab.png" });
});

test("diff context can be expanded and reviewed", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  }}));
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  const above = page.getByRole("button", { name: "Show 10 lines above", exact: true });
  const below = page.getByRole("button", { name: "Show 10 lines below", exact: true });
  await expect(above).toHaveText("");
  await expect(below).toHaveText("");
  await expect(below).toBeVisible();
  await page.screenshot({ path: "test-results/diff-context-arrows.png" });
  await above.click();
  const row = page.locator(".dv-l").filter({ hasText: "const value7 = 7;" });
  await expect(row).toBeVisible();
  await row.locator(".dv-g").click();
  await expect(page.getByPlaceholder("Comment on this line")).toBeVisible();
  await page.screenshot({ path: "test-results/expanded-diff-context.png" });
});

test("returning to a session restores its expanded diff and pane layout", async ({ page }) => {
  await openDesktop(page, true);
  const open = async (id: string) => page.evaluate(id => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }), id);
  await open("notice-one");
  await page.getByRole("tab", { name: "Changes", exact: true }).click();
  await page.keyboard.press("Shift+Escape");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
  await open("notice-two");
  await expect(page.getByRole("tab", { name: "Files", exact: true })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "Hide the pane", exact: true }).click();
  await open("notice-one");
  await expect(page.getByRole("tab", { name: "Changes", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
  await expect(page.locator(".agpane.zoomed")).toBeVisible();
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await open("notice-one");
  await expect(page.getByRole("tab", { name: "Changes", exact: true })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#agshell")).toHaveClass(/zoomed/);
});

test("returning and reloading reconnects the same terminal without stopping it", async ({ page }) => {
  await openDesktop(page, true, { terminal_open: {
    id: "shell-one", project: "", machine_id: "build-box", machine_name: "build-box", name: "shell",
    command: "", cwd: "/srv/project", state: "running", code: null, started_at: 1,
  } });
  const open = async (id: string) => page.evaluate(id => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id,
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }), id);
  const attaches = () => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "terminal_attach").length);
  await open("notice-one");
  await page.getByRole("button", { name: "+ Terminal", exact: true }).click();
  await expect.poll(attaches).toBeGreaterThan(0);
  const tab = page.locator('[role="tab"][data-mtab^="term:"]');
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await open("notice-two");
  await open("notice-one");
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect.poll(attaches).toBeGreaterThan(1);
  const calls = await page.evaluate(() => window.__navigation.calls());
  expect(calls.filter(c => c.command === "terminal_open")).toHaveLength(1);
  expect(calls.filter(c => c.command === "terminal_close")).toHaveLength(0);
  await page.reload();
  await page.waitForFunction(() => window.__navigation.ready());
  await open("notice-one");
  await expect(tab).toHaveAttribute("aria-selected", "true");
  await expect.poll(attaches).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.__navigation.calls().filter(c => c.command === "terminal_open"))).toHaveLength(0);
  await tab.locator("[data-mclose]").click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "terminal_close").length)).toBe(1);
});
