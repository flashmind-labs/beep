import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

async function openFile(page: Page, editable: boolean) {
  const text = "first match\r\nsecond match\r\n";
  await openDesktop(page, true, {
    agent_tree: { folders: [{ path: "", entries: [{ name: "notes.txt", folder: false, size: text.length, ignored: false, change: null }], more: 0, git: true }], truncated: false },
    agent_tree_text: { size: text.length, text, cut: false, binary: false, editable },
    agent_tree_edit: null,
  });
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="notes.txt"]').click();
  await expect(page.locator(".agcode pre")).toContainText("second match");
}

for (const editable of [true, false]) {
  test(`Find searches the focused ${editable ? "editor" : "read-only file"} and keeps Escape in the pane`, { tag: "@windows" }, async ({ page }) => {
    await openFile(page, editable);
    if (editable) await page.getByRole("button", { name: "Edit", exact: true }).click();
    const modifier = await page.evaluate(() => /Mac/.test(navigator.userAgent) ? "Meta" : "Control");
    await page.keyboard.press(`${modifier}+f`);
    const field = page.getByRole("textbox", { name: "Find", exact: true });
    await expect(field).toBeFocused();
    await field.pressSequentially("match");
    await field.press("Enter");
    await expect(page.locator(".cm-searchMatch-selected")).toHaveText("match");
    await field.press("Enter");
    await expect(page.locator(".cm-line").filter({ hasText: "second" }).locator(".cm-searchMatch-selected")).toHaveText("match");
    await expect(page.getByRole("search", { name: "Search conversation" })).toHaveCount(0);
    await field.press("Escape");
    await expect(field).toHaveCount(0);
    if (editable) await expect(page.locator(".cm-content")).toBeFocused();
    else await expect(page.locator(".agcode pre")).toBeVisible();
  });
}

test("Vim toggles on a draft, searches, edits and writes the current file", { tag: "@windows" }, async ({ page }) => {
  await openFile(page, true);
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.locator(".agcm .cm-content");
  await expect(editor).toBeFocused();
  await page.keyboard.type("draft ");
  const vim = page.getByRole("button", { name: "Vim", exact: true });
  await vim.click();
  await expect(page.locator(".cm-vim-panel")).toHaveText("--NORMAL--");
  await expect(editor).toBeFocused();
  const modifier = await page.evaluate(() => /Mac/.test(navigator.userAgent) ? "Meta" : "Control");
  await page.keyboard.press(`${modifier}+f`);
  const find = page.getByRole("textbox", { name: "Find", exact: true });
  await expect(find).toBeFocused();
  await find.press("Escape");
  await expect(editor).toBeFocused();
  await page.keyboard.type("0dw");
  await expect(editor).toContainText("first match");
  await expect(editor).not.toContainText("draft");
  await page.keyboard.type("iupdated ");
  await page.keyboard.press("Escape");
  await expect(page.locator(".cm-vim-panel")).toHaveText("--NORMAL--");
  await page.keyboard.press("Escape");
  await expect(editor).toBeFocused();
  await page.keyboard.type(":w");
  await page.keyboard.press("Enter");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree_edit").map(call => call.args.action)))
    .toEqual([{ Write: { path: "notes.txt", text: "updated first match\r\nsecond match\r\n" } }]);
  await page.getByRole("button", { name: "Done", exact: true }).click();
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(vim).toHaveAttribute("aria-pressed", "true");
  await vim.click();
  await page.keyboard.type("x");
  await expect(editor).toContainText("xupdated first match");
});
