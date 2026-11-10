import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("command suggestions keep focus and insert the selected command", async ({ page }) => {
  const commands = [{ name: "review", description: "Review work", hint: "[path]" }, { name: "run", description: "Run checks", hint: "" }];
  await openDesktop(page, true, { agent_commands: commands });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const field = page.locator("#agmsg");
  await expect(field).toBeVisible();
  await field.fill("/");
  const menu = page.locator(".cm-menu");
  await expect(menu.getByRole("option")).toHaveCount(2);
  await expect(field).toBeFocused();
  await field.press("ArrowDown");
  await expect(menu.getByRole("option", { selected: true })).toContainText("/run");
  await field.press("Enter");
  await expect(field).toHaveValue("/run ");
  await expect(field).toBeFocused();
  await field.fill("/re");
  await menu.getByRole("option", { name: /review/ }).click();
  await expect(field).toHaveValue("/review ");
  await expect(field).toBeFocused();
});

test("late command results cannot remount a removed composer", async ({ page }) => {
  await openDesktop(page, true, { agent_commands: [{ name: "review", description: "Review work", hint: "" }] });
  await page.evaluate(() => { window.__navigation.hold("agent_commands"); window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }); });
  await page.locator("#agmsg").fill("/");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_commands").length)).toBe(1);
  await page.evaluate(() => window.__navigation.open("chats"));
  await page.evaluate(() => window.__navigation.release("agent_commands"));
  await expect(page.locator(".cm-menu")).toHaveCount(0);
});

test("agent file picker preserves composer focus and attaches an image", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const input = page.locator("#agmsg"), picker = page.getByRole("button", { name: "Attach files" });
  await input.focus();
  await picker.dispatchEvent("pointerdown", { button: 0 });
  await expect(input).toBeFocused();
  await page.locator('#agfield input[type="file"]').setInputFiles({ name: "example.png", mimeType: "image/png", buffer: Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64") });
  await expect(page.locator("#agatt")).toContainText("example.png");
  await expect(input).toBeFocused();
});
