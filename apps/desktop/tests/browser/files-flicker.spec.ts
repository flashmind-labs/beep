import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

type Change = "untracked" | "added" | "modified" | "conflicted" | null;
const entry = (name: string, folder = false, change: Change = null) => ({ name, folder, size: 1, ignored: false, change });
const tree = (top: ReturnType<typeof entry>[], src: ReturnType<typeof entry>[]) => ({ folders: [{ path: "", entries: top, more: 0, git: true }, { path: "src", entries: src, more: 0, git: true }], truncated: false });
const session = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
/// The `files` topic of the session's folder.
const FOLDER = "files.%2Fsrv%2Fproject";

/// Records what an update does to the tree: rows taken out, messages and spinners put in.
const watch = (page: Page) => page.evaluate(() => {
  const log: string[] = [];
  const tree = document.querySelector('[role="tree"][aria-label="Working files"]');
  if (!tree) throw new Error("no tree");
  new MutationObserver(records => {
    for (const record of records) {
      for (const node of record.removedNodes) if (node instanceof HTMLElement && node.matches("[data-fpath]")) log.push(`removed ${node.dataset.fpath}`);
      for (const node of record.addedNodes) {
        if (!(node instanceof HTMLElement)) continue;
        if (node.matches(".agpmsg") || node.querySelector(".agpmsg")) log.push(`message ${node.textContent}`);
        if (node.matches(".spinner, [class*=spin]") || node.querySelector(".spinner, [class*=spin]")) log.push("spinner");
      }
    }
  }).observe(tree, { childList: true, subtree: true });
  (window as unknown as { __flicker: string[] }).__flicker = log;
});
const flicker = (page: Page) => page.evaluate(() => (window as unknown as { __flicker: string[] }).__flicker);

test("the file tree changes only what changed as the agent works", async ({ page }) => {
  const names = Array.from({ length: 30 }, (_, i) => `file${String(i).padStart(2, "0")}.txt`);
  await openDesktop(page, true, { agent_tree: tree([entry("src", true), ...names.map(n => entry(n))], [entry("main.ts")]) });
  await page.evaluate(value => window.__navigation.open({ agent: value }), session);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const files = page.getByRole("tree", { name: "Working files" });
  await expect(files.locator('[data-fpath="file00.txt"]')).toBeVisible();
  await files.locator('[data-fpath="src"]').click();
  await expect(files.locator('[data-fpath="src/main.ts"]')).toBeVisible();
  await files.locator('[data-fpath="file03.txt"]').click({ modifiers: ["Meta"] });
  await files.evaluate(element => { element.style.flex = "none"; element.style.height = "200px"; element.scrollTop = 120; });
  // The rows drawn follow the new size before the updates start.
  const drawn = () => files.evaluate(element => new Promise<string>(done => requestAnimationFrame(() => requestAnimationFrame(() => done([...element.querySelectorAll<HTMLElement>("[data-fpath]")].map(row => row.dataset.fpath).join())))));
  let rows = "";
  await expect.poll(async () => { const was = rows; rows = await drawn(); return rows === was; }).toBe(true);
  const scroll = await files.evaluate(element => element.scrollTop);
  await files.locator('[data-fpath="file05.txt"]').evaluate(element => { element.dataset.identity = "kept"; });
  await watch(page);

  // Each round the agent changes a file, adds one past the end and says the folder changed.
  for (let round = 1; round <= 4; round++) {
    const added = Array.from({ length: round }, (_, i) => entry(`zz${i}.txt`, false, "untracked"));
    await page.evaluate(value => window.__navigation.respond("agent_tree", value), tree([entry("src", true, "modified"), ...names.map((n, i) => entry(n, false, i === round ? "modified" : null)), ...added], [entry("main.ts", false, "modified")]));
    await page.evaluate(([topic, revision]) => window.__navigation.publish(topic, revision), [FOLDER, 1000 + round] as const);
    await page.evaluate(value => window.__navigation.open({ agent: value }), { ...session, last_activity: round + 1, doing: `step ${round}` });
    await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree" && call.args.fresh === true).length), { timeout: 5000 }).toBe(round);
    await expect(files.locator(`[data-fpath="file${String(round).padStart(2, "0")}.txt"] .gm`)).toHaveText("M");
  }

  expect(await flicker(page)).toEqual([]);
  await expect(files.locator('[data-fpath="file05.txt"]')).toHaveAttribute("data-identity", "kept");
  await expect(files.locator('[data-fpath="file03.txt"]')).toHaveAttribute("aria-selected", "true");
  await expect(files).toHaveJSProperty("scrollTop", scroll);
});

test("a folder of thousands of files draws only the rows in view and reveals one further down", async ({ page }) => {
  const names = Array.from({ length: 5000 }, (_, i) => `file${String(i).padStart(4, "0")}.txt`);
  await openDesktop(page, true, { agent_tree: { folders: [{ path: "", entries: names.map(n => entry(n)), more: 0, git: true }], truncated: false } });
  await page.evaluate(value => window.__navigation.open({ agent: value }), session);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const files = page.getByRole("tree", { name: "Working files" });
  await expect(files.locator('[data-fpath="file0000.txt"]')).toBeVisible();
  expect(await files.locator("[data-fpath]").count()).toBeLessThan(150);

  // The keyboard walks past the rows drawn, and End goes to the last one.
  await files.locator('[data-fpath="file0000.txt"]').click({ modifiers: ["Meta"] });
  await files.press("End");
  await expect(files.locator('[data-fpath="file4999.txt"]')).toBeInViewport();
  await expect(files.locator('[data-fpath="file4999.txt"]')).toHaveAttribute("aria-selected", "true");
  await expect(files).toHaveAttribute("aria-activedescendant", "agfile-file4999.txt");
  await expect(files.locator('[data-fpath="file0000.txt"]')).toHaveCount(0);
  expect(await files.locator("[data-fpath]").count()).toBeLessThan(150);
  await files.press("ArrowUp");
  await expect(files.locator('[data-fpath="file4998.txt"]')).toHaveAttribute("aria-selected", "true");

  // The filter narrows the rows from all of them.
  await page.getByRole("textbox", { name: "Filter files" }).fill("file2500");
  await expect(files.locator("[data-fpath]")).toHaveCount(1);
  await expect(files.locator('[data-fpath="file2500.txt"]')).toBeVisible();
});
