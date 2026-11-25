import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const listing = (names: string[]) => ({ folders: [{ path: "", entries: names.map(name => ({ name, folder: false, size: 1, ignored: false, change: null })), more: 0 }], truncated: false });
const session = (id: string) => ({ kind: "agent", machine_id: "build-box", machine_name: "build-box", id, agent: "codex", title: id, cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false });
/// The `files` topic of the sessions' folder.
const FOLDER = "files.%2Fsrv%2Fproject";

const reads = (page: Page, command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).length, command);
const open = (page: Page, id: string) => page.evaluate(value => window.__navigation.open({ agent: value }), session(id));

test("returning to a session draws Changes and Files as they were without reading again", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["a.txt", "b.txt"]) });
  await open(page, "notice-one");
  const files = page.getByRole("tab", { name: "Files", exact: true }), changes = page.getByRole("tab", { name: "Changes", exact: true });
  const diff = page.locator('.dv-file[data-path="sample.ts"]'), row = page.locator('[data-fpath="a.txt"]');
  await changes.click();
  await expect(diff).toBeVisible();
  await files.click();
  await expect(row).toBeVisible();
  const diffs = await reads(page, "work_diff"), trees = await reads(page, "agent_tree");

  // A read now would leave its placeholder up.
  await page.evaluate(() => { window.__navigation.hold("work_diff"); window.__navigation.hold("agent_tree"); });
  await page.evaluate(() => window.__navigation.open("home"));
  await open(page, "notice-one");
  await expect(row).toBeVisible();
  await expect(page.getByText("Reading the folder")).toHaveCount(0);
  await changes.click();
  await expect(diff).toBeVisible();
  await expect(page.getByText("Reading the changes")).toHaveCount(0);
  await files.click();
  await expect(row).toBeVisible();
  expect(await reads(page, "work_diff")).toBe(diffs);
  expect(await reads(page, "agent_tree")).toBe(trees);

  // Another session's diff is its own; coming back to the first reads nothing.
  await page.evaluate(() => { window.__navigation.release("work_diff"); window.__navigation.release("agent_tree"); });
  await open(page, "notice-two");
  await changes.click();
  await expect(diff).toBeVisible();
  expect(await reads(page, "work_diff")).toBe(diffs + 1);
  await open(page, "notice-one");
  await changes.click();
  await expect(diff).toBeVisible();
  expect(await reads(page, "work_diff")).toBe(diffs + 1);
  expect(await reads(page, "agent_tree")).toBe(trees);
});

test("a files event reads the shown diff once and the tree once it shows", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["a.txt"]) });
  await open(page, "notice-one");
  const files = page.getByRole("tab", { name: "Files", exact: true }), changes = page.getByRole("tab", { name: "Changes", exact: true });
  await expect(page.locator('[data-fpath="a.txt"]')).toBeVisible();
  await changes.click();
  await expect(page.locator('.dv-file[data-path="sample.ts"]')).toBeVisible();
  const diffs = await reads(page, "work_diff"), trees = await reads(page, "agent_tree");

  await page.evaluate(topic => window.__navigation.publish(topic, 100), FOLDER);
  await expect.poll(() => reads(page, "work_diff"), { timeout: 5000 }).toBe(diffs + 1);
  // The same revision again, as a new subscription is primed with it, is no change.
  await page.evaluate(topic => window.__navigation.publish(topic, 100), FOLDER);
  await page.waitForTimeout(500);
  expect(await reads(page, "work_diff")).toBe(diffs + 1);
  expect(await reads(page, "agent_tree")).toBe(trees);

  // Git files the diff never reads change nothing in it; a file of the checkout does, at once.
  await page.evaluate(topic => window.__navigation.publish(topic, 101, { Sessions: { Tree: { changed: [".git/FETCH_HEAD"] } } }), FOLDER);
  await page.waitForTimeout(500);
  expect(await reads(page, "work_diff")).toBe(diffs + 1);
  await page.evaluate(topic => window.__navigation.publish(topic, 102, { Sessions: { Tree: { changed: ["sample.ts"] } } }), FOLDER);
  await expect.poll(() => reads(page, "work_diff"), { timeout: 1000 }).toBe(diffs + 2);

  const folder = { name: "src", folder: true, size: 0, ignored: false, change: null };
  const changed = listing(["a.txt", "new.txt"]);
  changed.folders[0].entries.push(folder);
  await page.evaluate(value => window.__navigation.respond("agent_tree", value), changed);
  await files.click();
  await expect(page.locator('[data-fpath="new.txt"]')).toBeVisible();
  expect(await reads(page, "agent_tree")).toBe(trees + 1);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree").at(-1)?.args.fresh)).toBe(true);

  // Only that read went past the native cache: opening a folder uses it again.
  await page.locator('[data-fpath="src"]').click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree").at(-1)?.args)).toMatchObject({ path: "src", fresh: false });
  await changes.click();
  expect(await reads(page, "work_diff")).toBe(diffs + 2);
});
