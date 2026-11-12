import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const machine = { id: "one", name: "Build box", online: true, can_host: true, seen_at: 0, version: "1", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
const home = { machines: [machine], projects: [], running: [], needs_you: [] };
const forge = { host: "github", github: true, owners: ["example", "other"], error: null };
async function add(page: Page) {
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "New project", exact: true }).click();
  return page.getByRole("dialog", { name: "New project", exact: true });
}
test("new project preserves input and focus while options change", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, project_forges: [forge] });
  const sheet = await add(page);
  const name = sheet.getByRole("textbox", { name: "Name" });
  await name.fill("example-project");
  await name.evaluate(el => { if (el instanceof HTMLInputElement) { el.setSelectionRange(2, 5); Reflect.set(window, "projectNameInput", el); } });
  await sheet.getByRole("button", { name: "Public", exact: true }).click();
  await expect(name).toHaveValue("example-project");
  expect(await name.evaluate(el => el === Reflect.get(window, "projectNameInput"))).toBe(true);
  await expect(sheet.getByRole("button", { name: "Public", exact: true })).toBeFocused();
  await sheet.getByRole("button", { name: "example", exact: true }).click();
  await page.getByRole("menuitem", { name: "other", exact: true }).click();
  await expect(sheet.getByRole("button", { name: "other", exact: true })).toBeFocused();
  await sheet.getByRole("button", { name: "Create", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "create_project").map(call => call.args))).toEqual([{ name: "example-project", repository: { host: "github", owner: "other", private: false }, machineIds: ["one"] }]);
});

test("a stale forge response cannot replace a reopened form", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, project_forges: [forge] });
  await page.evaluate(() => window.__navigation.holdNext("project_forges"));
  let sheet = await add(page);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "project_forges").length)).toBe(1);
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  await page.evaluate(() => window.__navigation.respond("project_forges", []));
  sheet = await add(page);
  await expect(sheet.getByRole("button", { name: "Local only" })).toBeVisible();
  await sheet.getByRole("textbox", { name: "Name" }).fill("kept-name");
  await page.evaluate(value => { window.__navigation.respond("project_forges", [value]); window.__navigation.release("project_forges"); }, forge);
  await expect(sheet.getByRole("textbox", { name: "Name" })).toHaveValue("kept-name");
  await expect(sheet.getByRole("button", { name: "GitHub" })).toHaveCount(0);
});

test("creation completes without replacing a newer project form", async ({ page }) => {
  await openDesktop(page, false, { center_home: home, project_forges: [] });
  let sheet = await add(page);
  await sheet.getByRole("textbox", { name: "Name" }).fill("first-project");
  await page.evaluate(() => window.__navigation.hold("create_project"));
  await sheet.getByRole("button", { name: "Create", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "create_project").length)).toBe(1);
  await page.keyboard.press("Escape");
  await expect(sheet).toHaveCount(0);
  sheet = await add(page);
  const name = sheet.getByRole("textbox", { name: "Name" });
  await name.fill("second-project");
  await page.evaluate(() => { window.__navigation.respond("create_project", { name: "first-project", url: "", imported: [], failed: [] }); window.__navigation.release("create_project"); });
  await expect(name).toHaveValue("second-project");
  await expect(sheet.getByRole("button", { name: "Create", exact: true })).toBeEnabled();
});

test("created project shows its destination and unmounts on close", async ({ page }) => {
  const project = { name: "example/project", url: "https://example.test/example/project", machine_id: "one", machine_name: "Build box", folder: "/srv/project", cloned: true, imported_at: 1 };
  await openDesktop(page, false, { center_home: home, project_forges: [forge], create_project: { name: "project", url: project.url, imported: [project], failed: [] } });
  const sheet = await add(page);
  await sheet.getByRole("textbox", { name: "Name" }).fill("project");
  await sheet.getByRole("button", { name: "Create", exact: true }).click();
  await expect(sheet.getByText("example/project created", { exact: true })).toBeVisible();
  await expect(sheet.getByText("On Build box in /srv/project", { exact: true })).toBeVisible();
  await sheet.evaluate(el => Reflect.set(window, "projectSheet", el));
  await sheet.getByRole("button", { name: "Open project", exact: true }).click();
  await expect(sheet).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => { const node: unknown = Reflect.get(window, "projectSheet"); return node instanceof HTMLElement ? node.isConnected : null; })).toBe(false);
});

test("unavailable credentials and failed clone remain actionable", async ({ page }) => {
  const broken = { host: "forge.example", github: false, owners: [], error: "Credentials unavailable" };
  await openDesktop(page, false, { center_home: home, project_forges: [broken], create_project: { name: "project", url: "https://example.test/repo", imported: [], failed: [{ machine_id: "one", machine_name: "Build box", why: "Clone failed" }] } });
  const sheet = await add(page);
  await sheet.getByRole("textbox", { name: "Name" }).fill("project");
  await sheet.getByRole("button", { name: "forge.example", exact: true }).click();
  await expect(sheet.getByText("Credentials unavailable")).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Create", exact: true })).toBeDisabled();
  await sheet.getByRole("button", { name: "Local only", exact: true }).click();
  await sheet.getByRole("button", { name: "Create", exact: true }).click();
  await expect(sheet.getByText("Build box: Clone failed")).toBeVisible();
  await sheet.getByRole("button", { name: "Clone…", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Import a project" })).toBeVisible();
});

test("local-only creation keeps exactly the selected online machine", async ({ page }) => {
  const second = { ...machine, id: "two", name: "Other box" };
  await openDesktop(page, false, { center_home: { ...home, machines: [machine, second] }, project_forges: [forge] });
  const sheet = await add(page);
  await sheet.getByRole("textbox", { name: "Name" }).fill("project");
  await sheet.getByRole("button", { name: "Public", exact: true }).click();
  await sheet.getByRole("button", { name: "Local only", exact: true }).click();
  await sheet.getByRole("button", { name: "Other box", exact: true }).click();
  await expect(sheet.getByRole("button", { name: "Build box", exact: true })).toHaveAttribute("aria-pressed", "false");
  await expect(sheet.getByRole("button", { name: "Other box", exact: true })).toHaveAttribute("aria-pressed", "true");
  await sheet.getByRole("button", { name: "Create", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "create_project").map(call => call.args))).toEqual([{ name: "project", repository: null, machineIds: ["two"] }]);
});

test("creation is disabled when no machine is online", async ({ page }) => {
  await openDesktop(page, false, { center_home: { ...home, machines: [{ ...machine, online: false }] }, project_forges: [forge] });
  const sheet = await add(page);
  await sheet.getByRole("textbox", { name: "Name" }).fill("project");
  await expect(sheet.getByText("None of your machines is online.")).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Create", exact: true })).toBeDisabled();
});
