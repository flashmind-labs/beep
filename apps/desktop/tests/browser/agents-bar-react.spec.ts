import { expect, test } from "@playwright/test";
import { bundleFixture, openDesktop } from "./harness.ts";

const rows = Array.from({ length: 20 }, (_, index) => ({ id: `agent-${index}`, machine_id: "build-box", machine_name: "Build box", agent: "codex", title: `Agent ${index}`, cwd: "/srv/project", status: "working", doing: "Working", started_at: index, last_activity: index, external: false, question: null }));
test("agent list refresh preserves its rows, focus and scroll", async ({ page }) => {
  await openDesktop(page, false, { agent_sessions: rows, session_moves: [] });
  await page.goto("https://app.test/index.html");
  const script = await bundleFixture("tests/browser/agents-bar-fixture.ts");
  await page.addScriptTag({ content: script });
  await page.evaluate(() => Reflect.get(window, "openTestAgents")());
  const list = page.locator(".dagscroll"), row = page.getByRole("option", { name: /Agent 0/ });
  await expect(row).toBeVisible();
  await row.focus();
  await row.evaluate(el => Reflect.set(window, "agentListRow", el));
  await list.evaluate(el => { el.style.height = "100px"; el.style.overflow = "auto"; el.scrollTop = 200; });
  const top = await list.evaluate(el => el.scrollTop);
  await page.evaluate(async updated => { window.__navigation.respond("agent_sessions", updated); await Reflect.get(window, "refreshTestAgents")(); }, rows.map((row, i) => i ? row : { ...row, doing: "Updated activity" }));
  await expect(row).toContainText("Updated activity");
  expect(await row.evaluate(el => el === Reflect.get(window, "agentListRow") && document.activeElement === el)).toBe(true);
  await expect(list).toHaveJSProperty("scrollTop", top);
});
