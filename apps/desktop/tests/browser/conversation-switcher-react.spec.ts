import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("conversation cycling keeps its focused list and keyed rows", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "First notification", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.keyboard.down("Alt");
  await page.keyboard.press("Tab");
  const list = page.getByRole("listbox", { name: "Active conversations" });
  await expect(list).toBeFocused();
  const options = list.getByRole("option");
  await expect(options).toHaveCount(2);
  await options.first().evaluate(el => Reflect.set(window, "conversationFirst", el));
  await page.keyboard.press("Tab");
  expect(await options.first().evaluate(el => el === Reflect.get(window, "conversationFirst"))).toBe(true);
  await expect(list).toBeFocused();
  await expect(list).toHaveAttribute("aria-activedescendant", /conversation-choice-/);
  await page.keyboard.press("Escape");
  await page.keyboard.up("Alt");
  await expect(list).toHaveCount(0);
});
