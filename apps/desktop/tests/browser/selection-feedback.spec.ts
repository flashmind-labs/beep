import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { openDesktop } from "./harness.ts";

test("message text, dialog text and editable fields allow selection", async ({ page }) => {
  await page.setContent(`<body>
    <div class="cc-view"><span id="label">Project settings</span><pre class="in-pre">Preview text</pre></div>
    <div class="dbody"><div class="dcard dcoder"><header id="header">Session title</header>
      <div class="dcoderlog"><div class="dcoderen agent"><time id="stamp">12:35</time><div class="reply-body"><p id="reply">Select this chat message</p></div></div>
        <div class="dcoderen user"><div class="said" id="prompt">Select this prompt</div></div>
        <div class="dcoderen tool"><div class="dmedia"><button><span id="control">Chat action</span></button></div></div>
      </div></div></div>
    <div role="dialog" aria-label="Example"><div class="am-msg" id="description">Select this dialog text</div><button id="action">Close</button></div>
    <input aria-label="Name" value="Editable text"><textarea aria-label="Prompt">Editable prompt</textarea>
    <div contenteditable="true" aria-label="Editor">Editable content</div>
  </body>`);
  for (const name of ["style.css", "center.css", "sheets.css", "diff.css"]) {
    const css = await readFile(new URL(`../../dist/${name}`, import.meta.url), "utf8");
    await page.addStyleTag({ content: css.replace(/@import[^;]+;/g, "") });
  }
  for (const selector of ["#label", ".in-pre", "#header", "#stamp", "#control", "#action"]) {
    for (const clickCount of [2, 3]) {
      await page.locator(selector).click({ clickCount });
      expect(await page.evaluate(() => getSelection()?.toString())).toBe("");
    }
  }
  for (const selector of ["#reply", "#prompt", "#description"]) {
    await page.locator(selector).dblclick({ position: { x: 8, y: 8 } });
    expect(await page.evaluate(() => getSelection()?.toString())).toBe("Select");
    await page.evaluate(() => getSelection()?.removeAllRanges());
  }
  for (const name of ["Name", "Prompt", "Editor"]) {
    const field = page.getByLabel(name, { exact: true });
    await expect(field).toHaveCSS("user-select", "text");
    await field.fill("Updated text");
    if (name === "Editor") await expect(field).toHaveText("Updated text");
    else await expect(field).toHaveValue("Updated text");
  }
});

test("triple-clicks stay within the prompt or reply paragraph", async ({ page }) => {
  await openDesktop(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      const value = await invoke<T>(command, args);
      if (command !== "agent_view" || !value || typeof value !== "object") return value;
      return { ...value, entries: [
        { kind: "user", text: "Does it have threads?\n\nOr only sessions?", html: "<p>Does it have threads?</p><p>Or only sessions?</p>", sent_at: 1794579242 },
        { kind: "tool", text: "Run: cargo build [completed]", sent_at: 1794579244 },
        { kind: "agent", text: "Only sessions. Each has its own history.", html: "<p>Only sessions.</p><p>Each has its own history.</p>", sent_at: 1794579245 },
        { kind: "user", text: "One more question.", html: "<p>One more question.</p>", sent_at: 1794579248 },
      ] } as T;
    };
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));

  const prompt = page.locator(".dcoderen.user").first();
  await prompt.click({ clickCount: 3, position: { x: 3, y: 3 } });
  expect(await page.evaluate(() => getSelection()?.toString())).toBe("");

  await page.locator(".dcoderen.agent > .entry-time").first().click({ clickCount: 3 });
  expect(await page.evaluate(() => getSelection()?.toString())).toBe("");

  await prompt.getByText("Or only sessions?").click({ clickCount: 3 });
  expect(await page.evaluate(() => getSelection()?.toString().trim())).toMatch(/^Does it have threads\?\n+Or only sessions\?$/);

  for (const paragraph of ["Only sessions.", "Each has its own history."]) {
    await page.getByText(paragraph, { exact: true }).click({ clickCount: 3, position: { x: 12, y: 8 } });
    expect(await page.evaluate(() => getSelection()?.toString().trim())).toBe(paragraph);
  }
});
