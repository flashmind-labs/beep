import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const child = { pid: 123, command: "sleep 300", started_at: 1, ports: [], left_group: false };

test("a live session reads existing children and follows starts and exits", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.stack ?? error.message));
  await openDesktop(page, true);
  await page.evaluate(row => window.__navigation.processes([row]), child);
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  const running = page.getByRole("tab", { name: /Running/ });
  await expect(running).toBeVisible();
  await running.click();
  await expect(page.locator(".agrunningrow")).toContainText("sleep 300");
  await page.evaluate(() => window.__navigation.processes([]));
  await expect(running).toHaveCount(0);
  await page.evaluate(row => window.__navigation.processes([row]), { ...child, command: "sleep 301" });
  await expect(running).toBeVisible();
  await running.click();
  await expect(page.locator(".agrunningrow")).toContainText("sleep 301");
  await page.locator('.agpane:visible [data-pact="fold"]').click();
  await page.evaluate(() => window.__navigation.processes([]));
  await page.locator(".agpstrip:visible").click();
  await expect(running).toHaveCount(0);
  expect(errors).toEqual([]);
});
