import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("palette search keeps its field and selected result across index refreshes", async ({ page }) => {
  const project = (id: string, title: string) => ({ kind: "project", id, title, detail: "Project", keys: [title.toLowerCase()] });
  const index = { actions: [], settings: [], code: [], projects: [project("one", "example/tool"), project("two", "example/example")], sessions: [], machines: [] };
  await openDesktop(page, false, { center_palette_index: index, center_find_prompts: [] });
  await page.getByRole("button", { name: /^Search/ }).click();
  const dialog = page.getByRole("dialog", { name: "Search" }), input = dialog.getByRole("combobox");
  await input.fill("Example");
  const options = dialog.getByRole("option");
  await expect(options).toHaveCount(2);
  await expect(options.first()).toContainText("example/example");
  await expect(options.first()).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowDown");
  await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
  await input.evaluate(element => { element.dataset.identity = "kept"; });
  await options.nth(1).evaluate(element => { element.dataset.identity = "kept"; });
  await page.evaluate(() => window.__navigation.emit("palette-changed", null));
  await expect(input).toHaveAttribute("data-identity", "kept");
  await expect(input).toBeFocused();
  await expect(options.nth(1)).toHaveAttribute("data-identity", "kept");
  await expect(options.nth(1)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("button", { name: /^Search/ })).toBeFocused();
});

test("the right arrow on a project lists its sessions and Enter opens one", async ({ page }) => {
  const session = (id: string, title: string, project: string) => ({
    kind: "agent_session", id: `build-box ${id}`, title, detail: `Codex · ${project} · build-box`, keys: [title.toLowerCase(), project], project,
    session: { machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title, cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false },
  });
  const index = {
    actions: [], settings: [], code: [], machines: [],
    projects: [{ kind: "project", id: "example/widget", title: "example/widget", detail: "build-box", keys: ["example/widget"] }],
    sessions: [session("one", "Fix the redirect", "example/widget"), session("two", "Tidy the docs", "example/widget"), session("three", "Another widget", "example/relay")],
  };
  await openDesktop(page, false, { center_palette_index: index, center_find_prompts: [] });
  await page.getByRole("button", { name: /^Search/ }).click();
  const dialog = page.getByRole("dialog", { name: "Search" }), options = dialog.getByRole("option");
  await dialog.getByRole("combobox").fill("widget");
  await expect(options.first()).toHaveText(/example\/widget/);
  await expect(dialog.locator(".foot")).toContainText("sessions");

  await page.keyboard.press("ArrowRight");
  await expect(options).toHaveText([/Fix the redirect/, /Tidy the docs/]);
  await expect(dialog.locator(".foot")).toContainText("back");

  await page.keyboard.press("ArrowLeft");
  await expect(options).toHaveCount(4);
  await expect(options.first()).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(dialog).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_view").map(call => JSON.stringify(call.args)))).toEqual([expect.stringContaining('"two"')]);
});
