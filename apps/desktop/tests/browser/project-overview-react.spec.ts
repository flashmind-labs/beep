import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

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
