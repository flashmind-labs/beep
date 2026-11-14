import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const skills = [{ name: "crawl", description: "Crawl a site" }, { name: "map", description: "List a site's URLs" }, { name: "scrape", description: "Scrape one page" }];
const libraries = [
  { name: "firecrawl", url: "https://git.example.com/tools/firecrawl-skills.git", revision: "a3f91c2d00", skills, global: "None", projects: { [projectName]: { Only: ["map"] } } },
  { name: "writing", url: "https://git.example.com/team/writing-skills", revision: "77de0b4aa1", skills: [{ name: "release-notes", description: "Write release notes" }], global: "All", projects: {} },
];
const responses = {
  settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false },
  mac_control: { id: "one", name: "build-box", enabled: false, status: "off", error: null, permissions: { screen_recording: false, accessibility: false } },
  list_machines: [], skill_sync_get: true, project_sync_get: true, beep_sync_over_syncthing_get: false,
  skill_libraries_list: libraries,
  skill_libraries_problems: [{ machine: "win-box", library: "firecrawl", skill: "map", place: projectName, message: "a skill there has this name" }],
  skill_libraries_find: [{ url: "https://git.example.com/acme/ui-kit-skills", name: "ui-kit-skills", skills: ["a", "b"], places: [{ machine: "build-box", folder: "~/.claude/skills" }] }],
  project_skills_get: { libraries, keep_out_of_git: true },
  project_git_auto_recovery: false, project_ignored_sync: { enabled: false, folders: [] }, project_syncthing_mode: null,
};
const calls = (page: import("@playwright/test").Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).map(call => call.args), command);

test("Settings > Skills lists libraries, installs skills and adopts found ones", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.locator('[data-goto="skills"]').click();
  const row = settings.locator('[data-library="firecrawl"]');
  await expect(row).toContainText("git.example.com/tools/firecrawl-skills");
  await expect(row).toContainText("In 1 project");
  await expect(row).toContainText("a3f91c2");
  await expect(row).toContainText("win-box: map in");
  await expect(settings.locator('[data-library="writing"]')).toContainText("Global on every machine");

  await row.getByRole("button", { name: "Skills", exact: true }).click();
  await expect(row).toContainText(projectName);
  await row.locator(".dskill-line", { hasText: "crawl" }).getByRole("button", { name: "Install globally" }).click();
  await expect.poll(() => calls(page, "skill_library_set_skill")).toEqual([{ name: "firecrawl", skill: "crawl", installed: true }]);

  await row.getByRole("button", { name: "More for firecrawl" }).click();
  await page.getByRole("menuitem", { name: "Install globally" }).click();
  await expect.poll(() => calls(page, "skill_library_set_global")).toEqual([{ name: "firecrawl", pick: "All" }]);

  await settings.getByRole("button", { name: "Find existing libraries" }).click();
  const found = settings.locator('[data-library="ui-kit-skills"]');
  await expect(found).toContainText("~/.claude/skills on build-box");
  await found.getByRole("button", { name: "Add as library" }).click();
  await expect.poll(() => calls(page, "skill_library_add")).toEqual([{ url: "https://git.example.com/acme/ui-kit-skills", global: true }]);
  await expect(found).toHaveCount(0);

  await settings.getByRole("button", { name: "+ Add a library" }).click();
  const input = settings.getByRole("textbox", { name: "Git URL" });
  await expect(input).toBeFocused();
  await input.fill(" https://git.example.com/acme/3d-skills ");
  await input.press("Enter");
  await expect.poll(() => calls(page, "skill_library_add")).toContainEqual({ url: "https://git.example.com/acme/3d-skills", global: false });
  await expect(input).toHaveCount(0);
});

test("a project's Skills sheet saves its picks", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.evaluate(name => window.__navigation.open({ projects: { project: name } }), projectName);
  await page.getByRole("button", { name: "More", exact: true }).click();
  await page.getByRole("menuitem", { name: "Skills…" }).click();
  const sheet = page.getByRole("dialog", { name: `Skills in ${projectName}` });
  await expect(sheet).toContainText("1 of 3 skills");
  await expect(sheet.getByRole("checkbox", { name: "writing" })).toBeDisabled();
  await expect(sheet.getByRole("checkbox", { name: "firecrawl" })).toHaveJSProperty("indeterminate", true);
  await sheet.getByRole("button", { name: "Choose skills" }).click();
  await sheet.getByRole("checkbox", { name: "scrape" }).check();
  await sheet.getByRole("checkbox", { name: "Keep out of Git" }).uncheck();
  await sheet.getByRole("button", { name: "Save" }).click();
  await expect(sheet).toHaveCount(0);
  expect(await calls(page, "project_skills_set")).toEqual([{ project: projectName, picks: { firecrawl: { Only: ["map", "scrape"] }, writing: "None" }, keepOutOfGit: false }]);
});
