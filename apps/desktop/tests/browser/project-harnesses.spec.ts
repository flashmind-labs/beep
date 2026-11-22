import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const responses = {
  settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
  list_machines: [], project_harnesses: null,
};
const calls = (page: import("@playwright/test").Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

test("a project's Harnesses sheet limits it to the harnesses picked", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await page.getByRole("button", { name: "More", exact: true }).click();
  await page.getByRole("menuitem", { name: "Harnesses" }).click();
  const sheet = page.getByRole("dialog", { name: `Harnesses in ${projectName}` });
  const chips = sheet.getByRole("group", { name: "Harnesses" });
  await expect(sheet).toContainText("Every harness may run in this project.");

  for (const name of ["Claude Code", "Codex", "Gemini CLI", "Antigravity", "OpenCode", "Hermes"]) await chips.getByRole("button", { name, exact: true }).click();
  await expect(chips.getByRole("button", { name: "flsh", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(chips.getByRole("button", { name: "Codex", exact: true })).toHaveAttribute("aria-pressed", "false");
  await sheet.getByRole("button", { name: "Save" }).click();
  await expect(sheet).toHaveCount(0);
  expect(await calls(page, "project_harnesses_save")).toEqual([{ project: projectName, harnesses: ["pi", "flsh"] }]);
});
