import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("header text updates in place without keeping its initial placeholder", async ({ page }) => {
  await openDesktop(page);
  const title = page.locator("#title");
  await expect(title).toHaveText("Home");
  await title.evaluate(element => Reflect.set(window, "headerTitle", element));
  await page.evaluate(() => window.__navigation.open("instructions"));
  await expect(title).toHaveText("Instructions");
  expect(await title.evaluate(element => element === Reflect.get(window, "headerTitle"))).toBe(true);
  await expect(page.locator("#pw-back")).toHaveCount(0);
});

test("project-window Back retains its button across page updates", async ({ page }) => {
  // Too narrow for the project's sessions, which go back to the project with its name.
  await page.setViewportSize({ width: 640, height: 800 });
  await openDesktop(page, false, { center_window_alone: { kind: "project", project: projectName } });
  await expect(page.locator("body")).toHaveClass(/pw/);
  const back = page.locator("#pw-back");
  await expect(back).toBeHidden();
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await expect(back).toBeVisible();
  await back.evaluate(element => Reflect.set(window, "projectBack", element));
  // Back is a child of the main column, ahead of the header row that the grid lays beside it.
  expect(await back.evaluate(element => element.parentElement?.id === "main" && element.nextElementSibling?.classList.contains("cc-top"))).toBe(true);
  await back.click();
  await expect(back).toBeHidden();
  expect(await back.evaluate(element => element === Reflect.get(window, "projectBack"))).toBe(true);
});
