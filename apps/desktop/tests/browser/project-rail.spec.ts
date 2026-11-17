import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("a project's session swaps the sidebar for the project's sessions in the same column", async ({ page }) => {
  const sessions = Array.from({ length: 4 }, (_, i) => ({ machine_id: "build-box", machine_name: "build-box", id: `past-${i}`, agent: "codex", title: `Past session ${i}`, cwd: "/srv/project", status: "ended", doing: null, started_at: 1, last_activity: 100 - i, external: false, question: null, assistant: false }));
  await openDesktop(page, false, { project_sessions: sessions });
  await page.evaluate(project => window.__navigation.open({ projects: { project, tile: "agents" } }), projectName);
  const win = page.locator(".cwin"), rail = page.locator("#rail"), side = page.locator("#side"), main = page.locator("#main");
  await expect(rail).toBeHidden();

  await page.locator(".pj-det .pv-row").filter({ hasText: "Past session 1" }).click();
  await expect(win).toHaveClass(/rail-on/);
  await expect(side).toBeHidden();
  const past = rail.locator(".pj-li").filter({ hasText: "Past session" });
  await expect(past).toHaveCount(4);
  await expect(rail.locator('.pj-li[aria-current="true"]')).toHaveText(/Past session 1/);
  const at = await main.boundingBox();

  // The sessions take the sidebar's width, which the grip sets and keeps.
  expect(Math.round((await rail.boundingBox())?.width ?? 0)).toBe(300);
  await page.locator("#side-grip").press("ArrowRight");
  await expect.poll(async () => Math.round((await rail.boundingBox())?.width ?? 0)).toBe(316);
  expect(await page.evaluate(() => localStorage.getItem("beep.sideWidth"))).toBe("316");
  await page.locator("#side-grip").press("ArrowLeft");

  await rail.getByPlaceholder("Filter sessions").fill("session 3");
  await expect(past).toHaveText([/Past session 3/]);

  await rail.locator(".cc-side-toggle").click();
  await expect(rail).toBeHidden();
  await expect(side).toBeVisible();
  expect(await main.boundingBox()).toEqual(at);
  await side.locator(".cc-side-toggle").click();
  await expect(rail).toBeVisible();
  await expect(side).toBeHidden();

  await rail.locator(".rail-back").click();
  await expect(rail).toBeHidden();
  await expect(side).toBeVisible();
});
