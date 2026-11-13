import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const note = (id: string, at: number) => ({ id, at, kind: "finished", title: id, text: "", source: "build-box", read: false, done: false, target: null });
test("notification keyboard focus survives updates and dismissals", async ({ page }) => {
  const notes = [note("first", 3), note("second", 2), note("third", 1)];
  await openDesktop(page, false, { notes_list: notes });
  await page.getByRole("button", { name: "Search", exact: true }).focus();
  const shortcut = test.info().project.name === "macOS" ? "Meta+b" : "Control+b";
  await page.keyboard.press(shortcut);
  const second = page.locator('.nt-row[data-id="second"]');
  await expect(page.locator('.nt-row[data-id="first"]')).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(second).toBeFocused();
  await second.evaluate(el => Reflect.set(window, "noticeRow", el));
  await page.evaluate(value => window.__navigation.emit("notes", value), [note("new", 4), ...notes]);
  await expect(second).toBeFocused();
  expect(await second.evaluate(el => el === Reflect.get(window, "noticeRow"))).toBe(true);
  await page.keyboard.press("ArrowDown");
  await page.locator('.nt-row[data-id="third"]').getByRole("button", { name: "Dismiss" }).click();
  await expect(second.getByRole("button", { name: "Dismiss" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator(".nt-pop")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Search", exact: true })).toBeFocused();
});

test("opening a build dismisses its notification and keeps the others", async ({ page }) => {
  const target = { projects: { project: "example/widget", run: { id: 7, host: "example.test" } } };
  const build = { ...note("build:7", 2), kind: "run", title: "Build passed", target };
  await openDesktop(page, false, { notes_list: [build, note("other", 1)] });
  await page.getByRole("button", { name: /Notifications/ }).click();
  await page.locator('.nt-row[data-id="build:7"]').press("Enter");
  await expect(page.locator(".nt-pop")).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "notes_dismiss").map(call => call.args))).toEqual([{ ids: ["build:7"] }]);
  await page.getByRole("button", { name: /Notifications/ }).click();
  await expect(page.getByText("Build passed")).toHaveCount(0);
  await expect(page.locator('.nt-row[data-id="other"]')).toBeVisible();
});

test("an empty open pane focuses notifications that arrive later", async ({ page }) => {
  await openDesktop(page, false, { notes_list: [] });
  const shortcut = test.info().project.name === "macOS" ? "Meta+b" : "Control+b";
  await page.keyboard.press(shortcut);
  await expect(page.locator(".nt-pop")).toBeFocused();
  await page.evaluate(value => window.__navigation.emit("notes", value), [note("first", 1)]);
  await expect(page.locator('.nt-row[data-id="first"]')).toBeFocused();
});

test("approval buttons submit once and can retry an error", async ({ page }) => {
  const approval = { ...note("approval", 1), kind: "asks", title: "Permission required" };
  await openDesktop(page, false, { notes_list: [approval] });
  await page.getByRole("button", { name: /Notifications/ }).click();
  const row = page.locator('.nt-row[data-id="approval"]');
  await page.evaluate(() => window.__navigation.rejectOnce("notes_answer"));
  await row.getByRole("button", { name: "Allow" }).click();
  await expect(row.locator(".nt-err")).toContainText("Requested test failure");
  await page.evaluate(() => window.__navigation.hold("notes_answer"));
  await row.getByRole("button", { name: "Deny" }).click();
  await expect(row.getByRole("button", { name: "Allow" })).toBeDisabled();
  await expect(row.getByRole("button", { name: "Deny" })).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "notes_answer").map(call => call.args))).toEqual([{ id: "approval", allow: true }, { id: "approval", allow: false }]);
  await page.evaluate(() => window.__navigation.release("notes_answer"));
});
