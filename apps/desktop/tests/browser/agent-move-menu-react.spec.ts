import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machines = [{ id: "build-box", name: "Build box", online: true, folder: "/srv/project", checking: false, checked_at: 1 }, { id: "other", name: "Other box", online: true, folder: "/srv/project", checking: false, checked_at: 1 }];
const hosts = [{ machine_id: "other", machine_name: "Other box", agents: ["codex"], workspace: "/srv/project", home: "/srv", capabilities: [] }];
async function openAgent(page: import("@playwright/test").Page) {
  await openDesktop(page, true, { project_machines: machines, agent_hosts: hosts, session_move_probe: true });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await expect(page.locator("#agmoveb")).toBeVisible();
}

test("move menu keeps its button through target discovery and sends the chosen machine", async ({ page }) => {
  await openAgent(page);
  await page.evaluate(() => window.__navigation.hold("project_machines"));
  const button = page.locator("#agmoveb");
  await button.focus();
  await button.evaluate(el => Reflect.set(window, "moveButton", el));
  await button.click();
  await expect(page.getByText("Checking machines…")).toBeVisible();
  await page.evaluate(() => window.__navigation.release("project_machines"));
  const target = page.getByRole("menuitem", { name: /Other box/ });
  await expect(target).toBeEnabled();
  expect(await button.evaluate(el => el === Reflect.get(window, "moveButton") && document.activeElement === el)).toBe(true);
  await target.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "session_move").map(call => call.args.to))).toEqual(["other"]);
});

test("a closed move menu ignores its late discovery", async ({ page }) => {
  await openAgent(page);
  await page.evaluate(() => window.__navigation.hold("project_machines"));
  const button = page.locator("#agmoveb");
  await button.click();
  await expect(page.getByText("Checking machines…")).toBeVisible();
  await button.click();
  await page.evaluate(() => window.__navigation.release("project_machines"));
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator(".mvm")).toHaveCount(0);
});

test("a move change reads the sessions without an ambient wait", async ({ page }) => {
  await openAgent(page);
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_sessions").length);
  await page.waitForTimeout(300);
  const before = await reads();
  await page.evaluate(() => window.__navigation.publish("moves.*", null));
  await expect.poll(reads, { timeout: 1000 }).toBeGreaterThan(before);
});

test("Esc closes the move menu first, and a session left and opened again shows it closed", async ({ page }) => {
  await openAgent(page);
  const button = page.locator("#agmoveb"), session = { kind: "agent", machine_id: "build-box", machine_name: "Build box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
  await button.click();
  await expect(page.getByRole("menuitem", { name: /Other box/ })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(button).toBeVisible();
  await expect(button).toHaveAttribute("aria-expanded", "false");

  await button.click();
  await expect(page.getByRole("menuitem", { name: /Other box/ })).toBeVisible();
  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(target => window.__navigation.open({ agent: target }), session);
  await expect(button).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator(".mvm")).toHaveCount(0);
  await button.click();
  await expect(page.getByRole("menuitem", { name: /Other box/ })).toBeVisible();
});
