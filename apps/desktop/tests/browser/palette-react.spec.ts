import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("palette search keeps its field and selected result across index refreshes", async ({ page }) => {
  const project = (id: string, title: string) => ({ kind: "project", id, title, detail: "Project", keys: [title.toLowerCase()] });
  const index = { actions: [], settings: [], code: [], projects: [project("one", "Example one"), project("two", "Example two")], sessions: [], machines: [] };
  await openDesktop(page, false, { center_palette_index: index, center_find_prompts: [] });
  await page.getByRole("button", { name: /^Search/ }).click();
  const dialog = page.getByRole("dialog", { name: "Search" }), input = dialog.getByRole("combobox");
  await input.fill("Example");
  const options = dialog.getByRole("option");
  await expect(options).toHaveCount(2);
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
