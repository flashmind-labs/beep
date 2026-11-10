import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("found project actions retain focus across overview refresh", async ({ page }) => {
  const found = [{ name: 'example/found', remote: 'https://example.test/found', places: [{ machine_id: 'build-box', machine_name: 'Build box', folder: '/srv/found' }], agents: [{ agent: 'codex', sessions: 2 }], last_activity: 0 }];
  await openDesktop(page, false, { found_projects: found });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const section = page.locator('#sec-found'), action = section.locator('.ap-row .acts button');
  await expect(section).toContainText('example/found');
  await action.focus();
  await action.evaluate(el => { (el as HTMLElement).dataset.identity = 'kept'; });
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'projects_on_top').length);
  await page.evaluate(() => window.__navigation.notify('projects'));
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === 'projects_on_top').length)).toBeGreaterThan(reads);
  await expect(action).toHaveAttribute('data-identity', 'kept');
  await expect(action).toBeFocused();
  await page.evaluate(() => window.__navigation.hold('import_project'));
  await action.click();
  await expect(action).toHaveText('Importing…');
  await expect(action).toBeDisabled();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'import_project'))).toBe(true);
  await page.evaluate(() => window.__navigation.release('import_project'));
});

test("overview card keeps focus through pin updates and opens by keyboard", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const card = page.locator('.pj-card').filter({ hasText: 'widget' });
  await expect(card).toBeVisible();
  await card.focus();
  await card.evaluate(el => { el.dataset.identity = 'kept'; });
  await card.getByRole('button', { name: 'Pin to top' }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(card.getByRole('button', { name: 'Unpin' })).toBeVisible();
  await expect(card).toHaveAttribute('data-identity', 'kept');
  await expect(card).toBeFocused();
  await card.press('Enter');
  await expect(page.locator('.pj-split')).toBeVisible();
  await expect(page.locator('.cc-view[data-project]')).toHaveAttribute('data-project', projectName);
});

test("group rename keeps its draft during refresh and saves on Enter", async ({ page }) => {
  await openDesktop(page, false, { project_groups: [{ id: 'group', name: 'Example group', paths: ['example'] }] });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const group = page.locator('.pj-org').filter({ hasText: 'Example group' });
  await group.getByRole('button', { name: 'Rename' }).click();
  const input = page.getByRole('textbox', { name: 'Group name' });
  await input.fill('Updated group');
  await page.locator('.pj-card').first().getByRole('button', { name: 'Pin to top' }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(input).toHaveValue('Updated group');
  await expect(input).toBeFocused();
  await input.press('Enter');
  await expect(page.locator('.pj-org').filter({ hasText: 'Updated group' })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === 'project_group_save' && call.args.name === 'Updated group'))).toBe(true);
});

test("overview refresh retains visible groups and a focused rename draft", async ({ page }) => {
  await openDesktop(page, false, { project_groups: [
    { id: "first", name: "First group", paths: ["example"] },
    { id: "second", name: "Second group", paths: ["other"] },
  ] });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  const first = page.locator(".pj-org").filter({ hasText: "First group" });
  const second = page.locator(".pj-org").filter({ hasText: "Second group" });
  await expect(second).toBeVisible();
  await second.evaluate(element => { element.dataset.identity = "kept"; });
  await first.getByRole("button", { name: "Rename" }).click();
  const input = page.getByRole("textbox", { name: "Group name" });
  await input.fill("Editing group");
  await page.locator(".pj-card").first().getByRole("button", { name: "Pin to top" }).evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(input).toHaveValue("Editing group");
  await expect(input).toBeFocused();
  await expect(second).toHaveAttribute("data-identity", "kept");
});

test("folder group editor keeps its draft and retries a failed save", async ({ page }) => {
  await openDesktop(page, false, { project_groups: [{ id: "group", name: "Example group", paths: ["work"] }] });
  await page.evaluate(() => window.__navigation.open({ projects: { project: null } }));
  await page.locator(".pj-org").filter({ hasText: "Example group" }).getByRole("button", { name: "Folders…" }).click();
  const sheet = page.getByRole("dialog", { name: "Folders of Example group" });
  const field = sheet.locator(".gp-field");
  await expect(field).toBeFocused();
  await field.fill("~/Projects/personal");
  await field.press("Enter");
  await expect(field).toHaveValue("");
  await expect(field).toBeFocused();
  await expect(sheet.locator(".gp-path")).toHaveCount(2);
  await sheet.locator(".gp-path").filter({ hasText: "personal" }).getByRole("button", { name: "Remove" }).click();
  await expect(field).toBeFocused();
  await field.fill("~/Projects/personal");
  await field.press("Enter");
  await page.evaluate(() => window.__navigation.rejectOnce("project_group_save"));
  await sheet.getByRole("button", { name: "Save group" }).click();
  await expect(sheet.locator(".am-msg.bad")).toContainText("Requested test failure");
  await expect(sheet.locator(".gp-path")).toHaveCount(2);
  await page.evaluate(() => window.__navigation.hold("project_group_save"));
  await sheet.getByRole("button", { name: "Save group" }).click();
  await expect(sheet.getByRole("button", { name: "Saving…" })).toBeDisabled();
  await page.keyboard.press("Escape");
  await expect(sheet).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_group_save").map(call => call.args))).toEqual([
    { id: "group", name: "Example group", paths: ["work", "personal"] },
    { id: "group", name: "Example group", paths: ["work", "personal"] },
  ]);
  await page.evaluate(() => { window.__navigation.respond("project_group_save", { id: "group", name: "Example group", paths: ["work", "personal"] }); window.__navigation.release("project_group_save"); });
  await expect(sheet).toHaveCount(0);
});
