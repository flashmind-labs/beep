import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const rows = Array.from({ length: 25 }, (_, i) => ({ session: `research-${i}`, topic: `Research ${i}`, status: "done", round: 1, sources: 2, started_at: 1000 + i, finished_at: 2000 + i }));
async function openResearch(page: import("@playwright/test").Page) {
  await openDesktop(page, false, { status: { link: "linked", signed_out: false, server: true, live: false, session: null }, open_session: [], session_agents: [], session_files: [], session_sources: [], list_questions: [], list_researches: rows });
  await page.goto("https://app.test/index.html");
  await expect(page.locator("#bar")).toBeVisible();
  await page.keyboard.press(test.info().project.name === "macOS" ? "Meta+r" : "Control+r");
  await expect(page.getByRole("listbox", { name: "Research" }).getByRole("option")).toHaveCount(rows.length);
}
test("research edits keep the input, caret, rows and scroll", async ({ page }) => {
  await openResearch(page);
  const input = page.locator("#rtopic"), list = page.getByRole("listbox", { name: "Research" });
  const row = list.getByRole("option").first();
  await input.fill("Research topic");
  await input.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 5); Reflect.set(window, "researchInput", el); } });
  await row.evaluate(el => Reflect.set(window, "researchRow", el));
  await list.evaluate(el => { el.style.height = "100px"; el.style.overflow = "auto"; el.scrollTop = 200; });
  const top = await list.evaluate(el => el.scrollTop);
  await input.press("x");
  await expect(input).toHaveValue("Rexrch topic");
  expect(await input.evaluate(el => el === Reflect.get(window, "researchInput") && document.activeElement === el && el instanceof HTMLInputElement && el.selectionStart === 3 && el.selectionEnd === 3)).toBe(true);
  expect(await row.evaluate(el => el === Reflect.get(window, "researchRow"))).toBe(true);
  await expect(list).toHaveJSProperty("scrollTop", top);
});

test("research keyboard selection and export target the chosen row", async ({ page }) => {
  await openResearch(page);
  const input = page.locator("#rtopic");
  await input.press("ArrowDown");
  await input.press("Tab");
  await page.evaluate(() => window.__navigation.respond("export_research", "/srv/research.pdf"));
  await page.getByRole("button", { name: "Export PDF" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "export_research").map(call => call.args))).toEqual([{ session: "research-1" }]);
});

test("shared bar pages size the window from their visible contents", async ({ page }) => {
  await openResearch(page);
  const measured = () => page.evaluate(() => {
    const host = document.querySelector<HTMLElement>("#bar > [data-bar-page]");
    if (!host) return 0;
    return 2 + [...host.children].reduce((height, element) => {
      if (!(element instanceof HTMLElement)) return height;
      return height + (element.matches(".dbody, .dlist, .dkeys, .agc-wrap") ? element.scrollHeight : element.offsetHeight);
    }, 0);
  });
  const lastHeight = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "resize_bar").map(call => Number(call.args.height)).at(-1) ?? 0);
  await expect.poll(measured).toBeGreaterThan(100);
  await expect.poll(async () => (await lastHeight()) === (await measured())).toBe(true);
  await page.evaluate(() => window.__navigation.emit("bar-shown", "chats"));
  await expect(page.getByRole("listbox", { name: "Chats" })).toBeVisible();
  await expect.poll(measured).toBeGreaterThan(100);
  await expect.poll(async () => (await lastHeight()) === (await measured())).toBe(true);
});
