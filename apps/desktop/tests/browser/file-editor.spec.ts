import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

async function openFile(page: Page, editable: boolean, name = "notes.txt", text = "first match\r\nsecond match\r\n") {
  await openDesktop(page, true, {
    agent_tree: { folders: [{ path: "", entries: [{ name, folder: false, size: text.length, ignored: false, change: null }], more: 0 }], truncated: false },
    agent_tree_text: { size: text.length, text, cut: false, binary: false, editable },
    agent_tree_edit: null,
  });
  await page.evaluate(() => window.__navigation.open({ agent: {
    kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example",
    cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false,
  } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator(`[data-fpath="${name}"]`).click();
  await expect(page.locator(".agcode pre")).toBeVisible();
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

test("fold a function with the gutter, keyboard and Vim", { tag: "@windows" }, async ({ page }) => {
  await openFile(page, true, "sample.ts", "function total() {\n  return 42;\n}\n");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.locator(".agcm .cm-content");
  const placeholder = editor.locator(".cm-foldPlaceholder");

  await page.getByTitle("Fold line", { exact: true }).click();
  await expect(placeholder).toBeVisible();
  await expect(editor).not.toContainText("return 42");
  await editor.focus();
  const modifier = await page.evaluate(() => /Mac/.test(navigator.platform) ? "Meta+Alt" : "Control+Shift");
  await editor.press(`${modifier}+BracketRight`);
  await expect(editor).toContainText("return 42");
  await editor.press(`${modifier}+BracketLeft`);
  await expect(placeholder).toBeVisible();

  await page.getByRole("button", { name: "Vim", exact: true }).click();
  await page.keyboard.type("za");
  await expect(editor).toContainText("return 42");
  await page.keyboard.type("za");
  await expect(placeholder).toBeVisible();
  await expect(page.getByRole("button", { name: "Save", exact: true })).toBeDisabled();
});

test("complete a local name and insert a matching bracket", async ({ page }) => {
  await openFile(page, true, "score.ts", "const totalScore = 42;\n");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.locator(".agcm .cm-content");
  await expect(editor.locator(".k")).toContainText("const");
  await editor.press("ArrowDown");
  await editor.pressSequentially("totalSc");
  await editor.press("Control+Space");
  await page.getByRole("option", { name: "totalScore", exact: true }).click();
  await editor.pressSequentially("(");
  await expect(editor.locator(".cm-line").last()).toHaveText("totalScore()");
  await editor.press("Backspace");
  await expect(editor.locator(".cm-line").last()).toHaveText("totalScore");
});

test("the editor colours a file's types, functions and macros", async ({ page }) => {
  await openFile(page, true, "lib.rs", "pub struct Point { x: i32 }\nfn main() { println!(\"{}\", 42); }\n");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.locator(".agcm .cm-content");
  await expect(editor.locator(".ty")).toHaveText(["Point", "i32"]);
  await expect(editor.locator(".fn")).toContainText(["main", "println"]);
});

test("replace two occurrences with multiple cursors", async ({ page }) => {
  await openFile(page, true, "notes.txt", "one two one");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  const editor = page.locator(".agcm .cm-content");
  await expect(editor).toBeFocused();
  const modifier = await page.evaluate(() => /Mac/.test(navigator.platform) ? "Meta" : "Control");
  await editor.press(`${modifier}+d`);
  await editor.press(`${modifier}+d`);
  await page.keyboard.type("three");
  await expect(editor).toHaveText("three two three");
});

test("record and replay a Vim macro and substitute across the file", async ({ page }) => {
  await openFile(page, true, "notes.txt", "one value\ntwo value");
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.getByRole("button", { name: "Vim", exact: true }).click();
  const editor = page.locator(".agcm .cm-content");
  await expect(editor).toBeFocused();
  await page.keyboard.type("qa0ciwnew");
  await page.keyboard.press("Escape");
  await page.keyboard.type("qj@a");
  await expect(editor.locator(".cm-line")).toHaveText(["new value", "new value"]);
  await page.keyboard.type(":%s/value/score/g");
  await page.keyboard.press("Enter");
  await expect(editor.locator(".cm-line")).toHaveText(["new score", "new score"]);
});
