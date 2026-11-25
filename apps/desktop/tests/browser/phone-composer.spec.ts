import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = {
  machine_id: "build-box", machine_name: "build-box", id: "busy-session", agent: "codex",
  title: "Busy session", cwd: "/srv/project", status: "working", external: false,
  doing: null, question: null, started_at: 1, last_activity: 1,
};
const steers = ["Also log each retry", "Then run clippy", "Keep the old name", "Open a PR when green"];
const view = {
  session, model: null, plan: [], earlier: 0, error: null, turns: 3,
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
  await expect(page.locator(".ag-tasks")).toHaveText(`${commands.length} commands running`);
  await expect(page.locator(".ag-queue .it")).toHaveCount(steers.length);
  // The steers were sent from this app, so its queue can edit them.
  await page.evaluate(({ agent, steers }) => window.__navigation.emit("outbox-changed", {
    machine_id: agent.machine_id, agent: agent.agent, session: agent.id, revision: 1, rows: [], resumed: null, pick: null,
    sent: steers.map((text, id) => ({ id, how: "steer", text, images: [], keys: [], state: "sent", reason: null })),
  }), { agent: session, steers });
}

test("a phone folds running commands and queued steers into a line each", async ({ page }) => {
  await openBusy(page);
  const line = page.locator(".ag-workline"), tasks = page.locator(".ag-tasks");
  await expect(line).toHaveText("4 queued");
  expect((await line.boundingBox())!.height).toBeLessThanOrEqual(44);
  expect((await tasks.boundingBox())!.height).toBeLessThanOrEqual(44);
  await expect(page.locator(".ag-queue .it").first()).toBeHidden();
  await page.screenshot({ path: "../../.artifacts/phone-composer-closed.png" });

  await line.click();
  await expect(line).toHaveAttribute("aria-expanded", "true");
  const list = page.locator(".ag-worklist");
  await expect(list.locator(".ag-queue .it").first()).toBeVisible();
  const box = (await list.boundingBox())!, lineBox = (await line.boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(lineBox.y);
  expect(box.height).toBeLessThanOrEqual(844 / 2 + 1);
  await page.screenshot({ path: "../../.artifacts/phone-composer-open.png" });

  await list.locator('.ag-queue [data-act="edit"]').first().click();
  await expect(page.locator("#agmsg")).toHaveValue(steers[0]);

  await line.click();
  await expect(list.locator(".ag-queue .it").first()).toBeHidden();
});

test("the desktop keeps the lists but caps their height", async ({ page }) => {
  await openDesktop(page, false, { agent_view: view, agent_tasks: [...tasks, ...tasks.map(t => ({ ...t, id: `${t.id}-more` }))], agent_steer: [] });
  await page.evaluate(agent => window.__navigation.open({ agent: { ...agent, kind: "agent" } }), session);
  await expect(page.locator(".ag-tasks")).toHaveText(`${commands.length * 2} commands running`);
  await expect(page.locator(".ag-workline")).toBeHidden();
  await expect(page.locator(".ag-queue .it").first()).toBeVisible();
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

test("moving between fields with the keyboard up undoes the scroll iOS gives a clipped box", async ({ page }) => {
  await openBusy(page);
  await page.evaluate(() => {
    const box = document.createElement("div");
    box.id = "clipped";
    box.style.cssText = "position:fixed;top:0;left:0;width:200px;height:100px;overflow:hidden";
    box.innerHTML = '<input id="one" type="text"><div style="height:400px"></div><input id="two" type="text">';
    document.body.append(box);
  });
  await page.locator("#one").focus();
  await page.evaluate(() => {
    const vv = window.visualViewport!;
    Object.defineProperty(vv, "height", { configurable: true, get: () => 480 });
    Object.defineProperty(vv, "offsetTop", { configurable: true, get: () => 0 });
    vv.dispatchEvent(new Event("resize"));
  });
  await expect(page.locator("body")).toHaveClass(/mb-kbd/);

  // The window stays the same size, as on iOS when focus moves with the keyboard up.
  await page.evaluate(() => {
    const box = document.getElementById("clipped")!;
    box.scrollTop = 300;
    document.getElementById("two")!.focus({ preventScroll: true });
  });
  await expect.poll(() => page.locator("#clipped").evaluate(box => box.scrollTop)).toBe(0);
});

test("a keyboard put away while the field keeps focus lets the field go and the page back", async ({ page }) => {
  await openBusy(page);
  const field = page.locator("#agmsg");
  await field.focus();
  const viewport = (height: number, top: number) => page.evaluate(([height, top]) => {
    const vv = window.visualViewport!;
    Object.defineProperty(vv, "height", { configurable: true, get: () => height });
    Object.defineProperty(vv, "offsetTop", { configurable: true, get: () => top });
    vv.dispatchEvent(new Event("resize"));
  }, [height, top]);
  await viewport(480, 300);
  await expect(page.locator("body")).toHaveClass(/mb-kbd/);

  // iOS hides the keyboard without a blur, as when another app takes it.
  await viewport(844, 0);
  await expect(page.locator("body")).not.toHaveClass(/mb-kbd/);
  await expect(field).not.toBeFocused();
  expect((await page.locator(".cwin").boundingBox())!.y).toBe(0);
});

test("a tap on Send with the keyboard up sends and keeps the field", async ({ page }) => {
  await openBusy(page);
  const field = page.locator("#agmsg");
  await field.fill("Ship it");
  await field.focus();
  await page.locator("#agsend").click();
  await expect(field).toBeFocused();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(c => c.command === "agent_steer"))).toBe(true);
});
