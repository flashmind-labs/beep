import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

test("custom agents keep focus on redraw and retain prompt and start actions", async ({ page }) => {
  const machines = [{ id: "build-box", name: "build-box", online: true, folder: "/srv/project", checking: false, checked_at: 1 }];
  const agent = { name: "Reviewer", harness: "codex", scope: "project", description: "Reviews changes", model: null, tools: [], color: "blue", path: ".codex/agents/reviewer.md", prompt: "Review the work.", prompt_cut: false, error: null };
  await openDesktop(page, false, { project_machines: machines, custom_agents: [agent], agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }] });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  await page.locator('.pj-list .pj-li[title="Sessions"]').click();
  await page.getByRole('tab', { name: /^Agents/ }).click();
  const row = page.locator('#sec-custom-agents .ca-row');
  await expect(row).toContainText('Reviewer');
  await row.focus();
  await row.evaluate(el => { el.dataset.identity = 'kept'; });
  await page.getByRole('tab', { name: /^Agents/ }).evaluate(el => { if (el instanceof HTMLElement) el.click(); });
  await expect(row).toHaveAttribute('data-identity', 'kept');
  await expect(row).toBeFocused();
  await row.press('Enter');
  await expect(page.locator('.ca-prompt')).toHaveText('Review the work.');
  await expect(page.locator('.ca-prompt')).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(row).toBeFocused();
  await row.press('Enter');
  await page.getByRole('dialog', { name: 'Custom agent Reviewer' }).getByRole('button', { name: 'Start' }).click();
  await expect(page.getByRole('textbox', { name: 'Task for a coding agent' })).toHaveValue('Use the Reviewer agent to ');
});
