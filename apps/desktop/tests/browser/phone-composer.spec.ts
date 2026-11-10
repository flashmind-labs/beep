import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  machine_id: "build-box", machine_name: "build-box", id: "busy-session", agent: "codex",
  title: "Busy session", cwd: "/srv/project", status: "working", external: false,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const steers = ["Also log each retry", "Then run clippy", "Keep the old name", "Open a PR when green"];
const view = {
  session, model: null, plan: [], files: [], earlier: 0, last_message: "", error: null, turns: 3,
  entries: Array.from({ length: 30 }, (_, i) => ({ kind: i % 2 ? "assistant" : "user", text: `Message ${i}\n${"Conversation text. ".repeat(20)}` })),
  queued: steers.length, queued_messages: steers,
};
const commands = ["cargo test -p sync", "npm run dev", "cargo watch -x check", "tail -f node.log", "docker compose up", "just desktop-dev"];
const tasks = commands.map((name, i) => ({ id: `task-${i}`, name, description: name, state: "running", can_stop: true, output_file: null, summary: null, entry: null }));

async function openBusy(page: Page) {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDesktop(page, false, { agent_view: view, agent_tasks: tasks, agent_steer: [] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator("body")).toHaveClass(/mobile/);
  await expect(page.locator(".ag-task")).toHaveCount(commands.length);
  await expect(page.locator(".ag-queue .it")).toHaveCount(steers.length);
}

test("before: the unfolded lists fill a phone", async ({ page }) => {
  await openBusy(page);
  // The layout before the fold: the same page without the rules for the line and its list.
  await page.evaluate(() => {
    for (const sheet of document.styleSheets) {
      for (let i = sheet.cssRules.length - 1; i >= 0; i--) if (sheet.cssRules[i].cssText.includes("ag-work")) sheet.deleteRule(i);
    }
  });
  await page.addStyleTag({ content: ".ag-workline { display: none }" });
  await page.screenshot({ path: "../../.artifacts/phone-composer-before.png" });
});

test("a phone folds running commands and queued steers into one line", async ({ page }) => {
  await openBusy(page);
  const line = page.locator(".ag-workline");
  await expect(line).toHaveText("6 running, 4 queued");
  expect((await line.boundingBox())!.height).toBeLessThanOrEqual(44);
  await expect(page.locator(".ag-task").first()).toBeHidden();
  await expect(page.locator(".ag-queue .it").first()).toBeHidden();
  await page.screenshot({ path: "../../.artifacts/phone-composer-closed.png" });

  await line.click();
  await expect(line).toHaveAttribute("aria-expanded", "true");
  const list = page.locator(".ag-worklist");
  await expect(list.locator(".ag-task").first()).toBeVisible();
  const box = (await list.boundingBox())!, lineBox = (await line.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(lineBox.y);
  expect(box.height).toBeLessThanOrEqual(844 / 2 + 1);
  await page.screenshot({ path: "../../.artifacts/phone-composer-open.png" });

  await list.locator('[data-agtask="task-2"]').click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(c => c.command === "agent_steer").map(c => c.args.steer))).toContainEqual({ stop_task: { task: "task-2" } });

  await list.locator('.ag-queue [data-act="edit"]').first().click();
  await expect(page.locator("#agmsg")).toHaveValue(steers[0]);

  await line.click();
  await expect(list.locator(".ag-task").first()).toBeHidden();
});

test("the desktop keeps the lists but caps their height", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view, agent_tasks: [...tasks, ...tasks.map(t => ({ ...t, id: `${t.id}-more` }))], agent_steer: [] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator(".ag-task")).toHaveCount(commands.length * 2);
  await expect(page.locator(".ag-workline")).toBeHidden();
  await expect(page.locator(".ag-task").first()).toBeVisible();
  const list = (await page.locator(".ag-worklist").boundingBox())!;
  expect(list.height).toBeLessThanOrEqual(900 * 0.3 + 1);
});

test("the field and its buttons stay above the phone keyboard", async ({ page }) => {
  await openBusy(page);
  const field = page.locator("#agmsg");
  await field.focus();
  await page.evaluate(() => {
    const vv = window.visualViewport!;
    Object.defineProperty(vv, "height", { configurable: true, get: () => 480 });
    Object.defineProperty(vv, "offsetTop", { configurable: true, get: () => 0 });
    vv.dispatchEvent(new Event("resize"));
  });
  await expect(page.locator("body")).toHaveClass(/mb-kbd/);
  for (const part of [field, page.locator("#agsend"), page.locator(".ag-workline")]) {
    const box = (await part.boundingBox())!;
    expect(box.y).toBeGreaterThanOrEqual(0);
    expect(box.y + box.height).toBeLessThanOrEqual(480);
  }
  await expect.poll(() => page.locator("#agbody").evaluate(log => log.scrollHeight - log.scrollTop - log.clientHeight < 40)).toBe(true);
  await page.screenshot({ path: "../../.artifacts/phone-composer-keyboard.png" });
});
