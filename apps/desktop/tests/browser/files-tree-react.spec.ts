import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const listing = (names: string[]) => ({ folders: [{ path: "", entries: names.map(name => ({ name, folder: false, size: 1, ignored: false, change: null })), more: 0, git: true }], truncated: false });

test("file menu keeps focus as local actions arrive and ignores late results after closing", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["a.txt"]), local_file: { apps: [] } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const row = page.locator('[data-fpath="a.txt"]'); await expect(row).toBeVisible();
  await page.evaluate(() => window.__navigation.hold("local_file"));
  await row.click({ button: "right" });
  const menu = page.getByRole("menu", { name: "File actions" });
  const rename = menu.getByRole("menuitem", { name: "Rename..." }); await rename.focus();
  await page.evaluate(() => window.__navigation.release("local_file"));
  await expect(menu.getByRole("menuitem", { name: "Reveal in Finder" })).toBeVisible();
  await expect(rename).toBeFocused();
  await page.keyboard.press("Escape"); await expect(menu).toHaveCount(0);
  await page.evaluate(() => window.__navigation.hold("local_file"));
  await row.click({ button: "right" }); await expect(menu).toBeVisible();
  await page.keyboard.press("Escape");
  await page.evaluate(() => window.__navigation.release("local_file"));
  await expect(menu).toHaveCount(0);
});

test("file tree keeps its menu and rows during refresh, and confirms bulk deletion", async ({ page }, info) => {
  await openDesktop(page, true, { agent_tree: listing(["a.txt", "b.txt", "report.html"]) });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const tree = page.getByRole("tree", { name: "Working files" });
  const row = (name: string) => tree.locator("[data-fpath]").filter({ hasText: name });
  await expect(row("report.html")).toBeVisible();
  await row("report.html").click({ button: "right" });
  const menu = page.locator(".agctx");
  await expect(menu.locator('[data-fact="preview"]')).toBeVisible();
  await menu.evaluate(element => { element.dataset.identity = "kept"; });
  await row("a.txt").evaluate(element => { element.dataset.identity = "kept"; });
  await tree.evaluate(element => { element.style.maxHeight = "50px"; element.style.overflow = "auto"; element.scrollTop = 15; });
  const scroll = await tree.evaluate(element => element.scrollTop);
  await page.evaluate(() => window.__navigation.hold("agent_tree"));
  await page.locator('[data-frefresh]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect(menu).toHaveAttribute("data-identity", "kept");
  await page.evaluate(value => window.__navigation.respond("agent_tree", value), listing(["a.txt", "b.txt", "report.html", "new.txt"]));
  await page.evaluate(() => window.__navigation.release("agent_tree"));
  await expect(row("new.txt")).toBeVisible();
  await expect(menu).toHaveAttribute("data-identity", "kept");
  await expect(row("a.txt")).toHaveAttribute("data-identity", "kept");
  await expect(tree).toHaveJSProperty("scrollTop", scroll);
  await tree.evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await row("a.txt").click({ button: "right" });
  await expect(menu.locator('[data-fact="preview"]')).toHaveCount(0);
  await page.keyboard.press("Escape");
  await tree.evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  const modifier = info.project.name === "Windows" ? "Control" : "Meta";
  await row("a.txt").click({ modifiers: ["Meta"] });
  await row("b.txt").click({ modifiers: ["Meta"] });
  await expect(tree.locator('[aria-selected="true"]')).toHaveCount(2);
  await row("a.txt").click({ button: "right" });
  await expect(tree.locator('[aria-selected="true"]')).toHaveCount(2);
  await expect(menu).toContainText("Delete 2 items permanently");
  await tree.press(`${modifier}+Backspace`);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree_edit").length)).toBe(0);
  const dialog = page.getByRole("dialog").filter({ hasText: "cannot be undone" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("button", { name: "Delete 2 items" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().find(call => call.command === "agent_tree_edit")?.args)).toEqual({ machineId: "build-box", root: "/srv/project", action: { Delete: { paths: ["a.txt", "b.txt"] } } });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "agent_tree" && call.args.fresh === true))).toBe(true);
});

test("file tree keeps keyboard navigation, filtering and drag payloads", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["alpha.txt", "beta.txt"]) });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const tree = page.getByRole("tree", { name: "Working files" });
  await expect(tree.locator('[data-fpath="alpha.txt"]')).toBeVisible();
  await tree.focus();
  await tree.press("Home");
  await expect(tree).toHaveAttribute("aria-activedescendant", "agfile-alpha.txt");
  await tree.press("ArrowDown");
  await expect(tree).toHaveAttribute("aria-activedescendant", "agfile-beta.txt");
  const drag = await tree.locator('[data-fpath="beta.txt"]').evaluate(element => {
    const data = new DataTransfer();
    element.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: data }));
    return { path: data.getData("application/x-beep-path"), text: data.getData("text/plain") };
  });
  expect(drag).toEqual({ path: "beta.txt", text: "beta.txt" });
  const filter = page.getByRole("textbox", { name: "Filter files" });
  await filter.fill("beta");
  await expect(tree.locator('[data-fpath="alpha.txt"]')).toHaveCount(0);
  await expect(tree.locator('[data-fpath="beta.txt"]')).toBeVisible();
  await filter.press("Escape");
  await expect(filter).toHaveValue("");
  await expect(tree.locator('[data-fpath="alpha.txt"]')).toBeVisible();
});

test("switching worktrees ignores an older pending tree refresh", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    let hold = false;
    window.addEventListener("test-hold-tree", () => { hold = true; });
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_tree") return invoke<T>(command, args);
      const root = args?.root;
      if (root === "/srv/project" && hold) {
        Reflect.set(window, "treeReadPending", true);
        await new Promise<void>(resolve => window.addEventListener("test-release-tree", () => resolve(), { once: true }));
      }
      const name = root === "/srv/project" ? hold ? "stale.txt" : "main.txt" : "branch.txt";
      return { folders: [{ path: "", entries: [{ name, folder: false, size: 1, ignored: false, change: null }], more: 0, git: true }], truncated: false } as T;
    };
    window.__navigation.worktrees([{ path: "/srv/project", branch: "main", head: "abc1234", own: true }, { path: "/srv/project/.worktrees/fix", branch: "fix", head: "abc1234", own: false }]);
    window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } });
  });
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const tree = page.getByRole("tree", { name: "Working files" });
  await expect(tree.locator('[data-fpath="main.txt"]')).toBeVisible();
  await tree.evaluate(element => { element.dataset.identity = "kept"; });
  await page.evaluate(() => window.dispatchEvent(new Event("test-hold-tree")));
  await page.locator("[data-frefresh]").evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "treeReadPending"))).toBe(true);
  await page.locator(".agwt:visible").click();
  await page.getByRole("menuitemradio", { name: /fix/ }).click();
  await expect(tree.locator('[data-fpath="branch.txt"]')).toBeVisible();
  await page.evaluate(() => window.dispatchEvent(new Event("test-release-tree")));
  await expect(tree.locator('[data-fpath="stale.txt"]')).toHaveCount(0);
  await expect(tree.locator('[data-fpath="branch.txt"]')).toBeVisible();
  await expect(tree).toHaveAttribute("data-identity", "kept");
});

test("file preview keeps its controls while the HTML loads", async ({ page }) => {
  await openDesktop(page, true, {
    agent_tree: listing(["report.html"]),
    agent_tree_text: { size: 19, text: "<p>Hello world</p>", cut: false, binary: false },
    agent_artifact: { kind: "html", src: "data:text/html,%3Cp%3EHello%3C%2Fp%3E" },
  });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="report.html"]').click();
  const preview = page.locator("[data-preview]");
  await expect(preview).toBeVisible();
  await page.evaluate(() => window.__navigation.hold("agent_artifact"));
  await preview.click();
  await expect(page.getByText("Loading preview…")).toBeVisible();
  await expect(preview).toHaveText("Source");
  await expect(preview).toBeFocused();
  await page.evaluate(() => window.__navigation.release("agent_artifact"));
  await expect(page.locator('iframe[title="Preview of report.html"]')).toBeVisible();
  await expect(preview).toBeFocused();
  await expect(preview).toHaveText("Source");
  await preview.click();
  await expect(page.locator(".agcode")).toContainText("Hello world");
  await expect(preview).toBeFocused();
});

test("invalid 3D files show an error inside the retained model stage", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["model.glb"]), agent_file: { size: 1, mime: "model/gltf-binary", data: "AA==" } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="model.glb"]').click();
  await expect(page.locator(".agpstage.model .agpmsg.derr")).toContainText("Could not draw model.glb:");
  await expect(page.getByRole("button", { name: "Reset view" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Wireframe" })).toBeDisabled();
  await page.getByRole("button", { name: "Open", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_file_open").length)).toBe(1);
});

test("video and PDF tabs retain their controls and native media", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["clip.mp4", "report.pdf"]), agent_file: { size: 1, mime: "application/octet-stream", data: "AA==" } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="clip.mp4"]').click();
  const video = page.locator(".agpview:visible").filter({ has: page.getByRole("button", { name: "Open in QuickTime" }) });
  await expect(video.getByText("This video does not play here. Open it in QuickTime instead.")).toBeVisible();
  await video.getByRole("button", { name: "Open in QuickTime" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_file_open").length)).toBe(1);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="report.pdf"]').click();
  await expect(page.locator('iframe[title="report.pdf"]')).toBeVisible();
  await expect(page.getByRole("button", { name: "Open in Preview" })).toBeVisible();
});
