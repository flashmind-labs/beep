import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

test("only chat and editable fields allow text selection", async ({ page }) => {
  await page.setContent(`<body>
    <div class="cc-view"><span id="label">Project settings</span><pre class="in-pre">Preview text</pre></div>
    <div class="dbody"><div class="dcard dcoder"><header id="header">Session title</header>
      <div class="dcoderlog"><div class="dcoderen agent"><p id="reply">Select this chat message</p></div>
        <div class="dcoderen user"><div class="said" id="prompt">Select this prompt</div></div>
        <div class="dcoderen tool"><div class="dmedia"><button><span id="control">Chat action</span></button></div></div>
      </div></div></div>
    <input aria-label="Name" value="Editable text"><textarea aria-label="Prompt">Editable prompt</textarea>
    <div contenteditable="true" aria-label="Editor">Editable content</div>
  </body>`);
  for (const name of ["style.css", "center.css", "sheets.css", "diff.css"]) {
    const css = await readFile(new URL(`../../dist/${name}`, import.meta.url), "utf8");
    await page.addStyleTag({ content: css.replace(/@import[^;]+;/g, "") });
  }
  for (const selector of ["#label", ".in-pre", "#header", "#control"]) {
    await page.locator(selector).dblclick();
    expect(await page.evaluate(() => getSelection()?.toString())).toBe("");
  }
  for (const selector of ["#reply", "#prompt"]) {
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
