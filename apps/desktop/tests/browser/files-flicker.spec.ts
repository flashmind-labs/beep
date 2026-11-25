import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

type Change = "untracked" | "added" | "modified" | "conflicted" | null;
const entry = (name: string, folder = false, change: Change = null) => ({ name, folder, size: 1, ignored: false, change });
const tree = (top: ReturnType<typeof entry>[], src: ReturnType<typeof entry>[]) => ({ folders: [{ path: "", entries: top, more: 0 }, { path: "src", entries: src, more: 0 }], truncated: false });
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

test("a listing pushed with the folder's update replaces the rows without a read", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: tree([entry("src", true), entry("a.txt")], [entry("main.ts")]) });
  await page.evaluate(value => window.__navigation.open({ agent: value }), session);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const files = page.getByRole("tree", { name: "Working files" });
  await expect(files.locator('[data-fpath="a.txt"]')).toBeVisible();
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree").length);
  const before = await reads();

  const view = { command: "agent_tree", args: { machineId: "build-box", root: "/srv/project", path: "", ignored: false, deep: false }, value: tree([entry("src", true), entry("a.txt", false, "modified"), entry("b.txt", false, "untracked")], []) };
  await page.evaluate(([topic, value]) => window.__navigation.publish(topic, 2000, { views: [value] }), [FOLDER, view] as const);
  await expect(files.locator('[data-fpath="b.txt"] .gm')).toHaveText("U");
  await expect(files.locator('[data-fpath="a.txt"] .gm')).toHaveText("M");
  expect(await reads()).toBe(before);

  // An update with no listing, as an older node sends, reads the folder again.
  await page.evaluate(([topic]) => window.__navigation.publish(topic, 2001), [FOLDER] as const);
  await expect.poll(reads).toBeGreaterThan(before);
});

test("a folder of thousands of files draws only the rows in view and reveals one further down", async ({ page }) => {
  const names = Array.from({ length: 5000 }, (_, i) => `file${String(i).padStart(4, "0")}.txt`);
  await openDesktop(page, true, { agent_tree: { folders: [{ path: "", entries: names.map(n => entry(n)), more: 0 }], truncated: false } });
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

test("a session shown again draws its worktree's tree where it was left, from the first frame", async ({ page }) => {
  await openDesktop(page, false);
  await page.evaluate(([s, rows]) => {
    // Each worktree has its own files, so a tree of the wrong one shows.
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>): Promise<T> => {
      if (command !== "agent_tree") return invoke<T>(command, args);
      Reflect.set(window, "treeRoots", [...Reflect.get(window, "treeRoots") ?? [], args?.root]);
      const tag = args?.root === "/srv/project" ? "main" : "fix";
      const entries = Array.from({ length: rows }, (_, i) => ({ name: `${tag}${String(i).padStart(2, "0")}.txt`, folder: false, size: 1, ignored: false, change: null }));
      return structuredClone({ folders: [{ path: "", entries, more: 0 }], truncated: false }) as T;
    };
    window.__navigation.worktrees([{ path: "/srv/project", branch: "main", head: "abc1234", own: true, created: false }, { path: "/srv/project/.worktrees/fix", branch: "fix", head: "abc1234", own: false, created: true }]);
    // The agent works in the linked worktree.
    window.__navigation.respond("agent_view", { session: { ...s, question: null }, plan: [], entries: [{ kind: "user", text: "Fix the login" }, { kind: "tool", text: "Edit /srv/project/.worktrees/fix/login.ts [completed]" }], earlier: 0, error: null, queued: 0, queued_messages: [], turns: 1 });
    window.__navigation.open({ agent: s });
  }, [session, 80] as const);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const files = page.getByRole("tree", { name: "Working files" });
  await expect(files.locator('[data-fpath="fix00.txt"]')).toBeVisible();
  await files.evaluate(element => { element.scrollTop = 400; });
  await expect(files.locator('[data-fpath="fix20.txt"]')).toBeInViewport();

  // Every frame painted while the tree shows: its first row in view and its
  // scroll, read after layout and the page's resize observers, before paint.
  await page.evaluate(() => {
    const frames: string[] = [];
    (window as unknown as { __frames: string[] }).__frames = frames;
    const sample = () => {
      const tree = document.querySelector<HTMLElement>('[role="tree"][aria-label="Working files"]');
      if (!tree?.offsetParent) return;
      const top = tree.getBoundingClientRect().top;
      const first = [...tree.querySelectorAll<HTMLElement>("[data-fpath]")].filter(row => row.getBoundingClientRect().bottom > top + 1).sort((a, b) => a.getBoundingClientRect().top - b.getBoundingClientRect().top)[0];
      const seen = `${first?.dataset.fpath ?? tree.textContent} at ${Math.round(tree.scrollTop)}`;
      if (frames.at(-1) !== seen) frames.push(seen);
    };
    const frame = () => {
      // A new observer reports last in this frame's resize observations.
      const last = new ResizeObserver(() => { last.disconnect(); sample(); });
      last.observe(document.body);
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  });
  const frames = () => page.evaluate(() => (window as unknown as { __frames: string[] }).__frames);
  await expect.poll(frames).toEqual(["fix18.txt at 400"]);

  const reads = (command: string) => page.evaluate(name => window.__navigation.calls().filter(call => call.command === name).length, command);
  const lists = await reads("agent_worktrees"), views = await reads("agent_view");
  await page.evaluate(() => { Reflect.set(window, "treeRoots", []); window.__navigation.open("home"); });
  await expect(files).toBeHidden();
  await page.evaluate(value => window.__navigation.open({ agent: value }), session);
  await expect(files).toBeVisible();
  // The worktree list and the log are read again, and drawn.
  await expect.poll(() => reads("agent_worktrees")).toBeGreaterThan(lists);
  await expect.poll(() => reads("agent_view")).toBeGreaterThan(views);
  await page.evaluate(() => new Promise(done => requestAnimationFrame(() => requestAnimationFrame(done))));
  expect(await frames()).toEqual(["fix18.txt at 400"]);
  // It stayed on the worktree: the session's own folder was not read.
  expect(await page.evaluate(() => Reflect.get(window, "treeRoots"))).toEqual([]);
});
