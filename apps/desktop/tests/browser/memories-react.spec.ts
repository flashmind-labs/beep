import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

test("memories search, navigation and edits preserve the active draft", async ({ page }) => {
  const memories = Array.from({ length: 100 }, (_, i) => ({ id: `memory-${i}`, content: `Memory ${i}`, category: "General", source: "user", updated: "2026-01-01", valid_from: null, valid_until: null, quote: null }));
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_memories: memories, list_memory_scopes: [], list_memory_reads: [] });
  await page.evaluate(() => window.__navigation.open("memories"));
  const list = page.getByRole("listbox", { name: "Memories" });
  await expect(list.getByText("Memory 0", { exact: true })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await expect(list.locator('[data-i="1"]')).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Enter");
  const editor = page.getByRole("textbox", { name: "Memory", exact: true });
  await editor.fill("Changed memory");
  await editor.press("Enter");
  await expect(list.getByText("Changed memory", { exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "update_memory").length)).toBe(1);
  const search = page.getByPlaceholder("Search memories");
  await search.fill("Memory 99");
  await expect(list.getByText("Memory 99", { exact: true })).toBeVisible();
  await expect(page.locator("#mcount")).toHaveText("1 of 100");
});

test("long memory lists load later rows without replacing earlier rows", async ({ page }) => {
  const memories = Array.from({ length: 200 }, (_, i) => ({ id: `memory-${i}`, content: `Memory ${i}`, category: "General", source: "user", updated: "2026-01-01", valid_from: null, valid_until: null, quote: null }));
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, list_memories: memories, list_memory_scopes: [{ id: "memory-199", scope: { kind: "project", name: "Example" } }], list_memory_reads: [] });
  await page.evaluate(() => window.__navigation.open("memories"));
  const list = page.getByRole("listbox", { name: "Memories" });
  const first = list.locator('[data-i="0"]');
  await expect(first).toBeVisible();
  await first.evaluate(element => { element.dataset.identity = "kept"; });
  await list.evaluate(element => { element.scrollTop = element.scrollHeight; });
  await expect(list.getByText("Memory 199", { exact: true })).toBeVisible();
  await expect(first).toHaveAttribute("data-identity", "kept");
  await page.getByRole("button", { name: /Project Example/ }).click();
  await expect(page.locator("#mcount")).toHaveText("1 of 200");
  await expect(list.getByText("Memory 199", { exact: true })).toBeVisible();
});
