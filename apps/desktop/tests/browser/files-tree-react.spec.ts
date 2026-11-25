import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const listing = (names: string[]) => ({ folders: [{ path: "", entries: names.map(name => ({ name, folder: false, size: 1, ignored: false, change: null })), more: 0 }], truncated: false });

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

test("file tree keeps its menu and rows during refresh, and confirms bulk deletion", { tag: "@windows" }, async ({ page }, info) => {
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
  await expect(filter).toHaveAttribute("autocomplete", "off");
  await expect(filter).toHaveAttribute("autocorrect", "off");
  await expect(filter).toHaveAttribute("autocapitalize", "off");
  await filter.fill("beta");
  await expect(tree.locator('[data-fpath="alpha.txt"]')).toHaveCount(0);
  await expect(tree.locator('[data-fpath="beta.txt"]')).toBeVisible();
  await filter.press("Escape");
  await expect(filter).toHaveValue("");
  await expect(tree.locator('[data-fpath="alpha.txt"]')).toBeVisible();
});

test("file tree filter finds folder/file and lists folder/folder", async ({ page }) => {
  const nestedTree = {
    folders: [
      { path: "", entries: [{ name: "agents", folder: true, size: 0, ignored: false, change: null }], more: 0, git: true },
      { path: "agents", entries: [{ name: "session", folder: true, size: 0, ignored: false, change: null }, { name: "other", folder: true, size: 0, ignored: false, change: null }], more: 0, git: true },
      { path: "agents/session", entries: [{ name: "actor.rs", folder: false, size: 1, ignored: false, change: null }, { name: "state.rs", folder: false, size: 1, ignored: false, change: null }], more: 0, git: true },
      { path: "agents/other", entries: [{ name: "actor.rs", folder: false, size: 1, ignored: false, change: null }], more: 0, git: true },
    ],
    truncated: false,
  };
  await openDesktop(page, true, { agent_tree: nestedTree });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const tree = page.getByRole("tree", { name: "Working files" });
  const filter = page.getByRole("textbox", { name: "Filter files" });

  await filter.fill("session/actor");
  await expect(tree.locator('[data-fpath="agents/session/actor.rs"]')).toBeVisible();
  await expect(tree.locator('[data-fpath="agents/session/state.rs"]')).toHaveCount(0);
  await expect(tree.locator('[data-fpath="agents/other/actor.rs"]')).toHaveCount(0);

  await filter.fill("agents/session");
  await expect(tree.locator('[data-fpath="agents/session/actor.rs"]')).toBeVisible();
  await expect(tree.locator('[data-fpath="agents/session/state.rs"]')).toBeVisible();
  await expect(tree.locator('[data-fpath="agents/other"]')).toHaveCount(0);
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
      return { folders: [{ path: "", entries: [{ name, folder: false, size: 1, ignored: false, change: null }], more: 0 }], truncated: false } as T;
    };
    window.__navigation.worktrees([{ path: "/srv/project", branch: "main", head: "abc1234", own: true, created: false }, { path: "/srv/project/.worktrees/fix", branch: "fix", head: "abc1234", own: false, created: true }]);
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
  // An HTML file opens in its preview.
  await page.evaluate(() => window.__navigation.hold("agent_artifact"));
  await page.locator('[data-fpath="report.html"]').click();
  const preview = page.locator("[data-preview]");
  await expect(page.getByText("Loading preview…")).toBeVisible();
  await expect(preview).toHaveText("Source");
  await page.evaluate(() => window.__navigation.release("agent_artifact"));
  await expect(page.locator('iframe[title="Preview of report.html"]')).toBeVisible();
  await expect(preview).toHaveText("Source");
  await preview.click();
  await expect(page.locator(".agcode")).toContainText("Hello world");
  await expect(preview).toBeFocused();
  await preview.click();
  await expect(page.locator('iframe[title="Preview of report.html"]')).toBeVisible();
  await expect(preview).toBeFocused();
  await expect(preview).toHaveText("Source");
});

test("a folder event refreshes the open HTML preview after a same-size agent edit", async ({ page }) => {
  const text = (word: string) => ({ size: word.length, text: word, cut: false, binary: false, editable: true });
  const artifact = (word: string) => ({ kind: "html", src: `data:text/html,${word}` });
  const session = { kind: "agent" as const, machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false, question: null };
  await page.clock.install();
  const path = ".worktrees/site/index.html";
  const entries = [{ kind: "agent", text: `[Preview](${path})`, html: `<p><a href="${path}">Preview</a></p>` }];
  await openDesktop(page, false, {
    agent_view: { session, entries, earlier: 0, plan: [], error: null, turns: 1, queued: 0, queued_messages: [] },
    agent_tree_text: text("first"),
    agent_file_hashes: ["first"],
    agent_page_files: [],
    agent_artifact: artifact("first"),
  });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await page.getByRole("link", { name: "Preview", exact: true }).click();
  const body = page.frameLocator('iframe[title="Preview of index.html"]').locator("body");
  await expect(body).toHaveText("first");

  await page.evaluate(({ nextText, nextArtifact }) => {
    window.__navigation.respond("agent_file_hashes", ["later"]);
    window.__navigation.respond("agent_tree_text", nextText);
    window.__navigation.respond("agent_artifact", nextArtifact);
  }, { nextText: text("later"), nextArtifact: artifact("later") });

  const hashReads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_file_hashes").length);
  const before = await hashReads();
  await page.clock.fastForward(15_000);
  await page.clock.fastForward(1_000);
  expect(await hashReads()).toBe(before);
  await expect(body).toHaveText("first");

  const site = "files.%2Fsrv%2Fproject%2F%2Eworktrees%2Fsite";
  // A change to another file of the folder asks nothing.
  await page.evaluate(topic => window.__navigation.publish(topic, 1000, { Sessions: { Tree: { changed: ["notes.txt"] } } }), site);
  await page.clock.fastForward(1_000);
  expect(await hashReads()).toBe(before);
  await expect(body).toHaveText("first");

  await page.evaluate(topic => window.__navigation.publish(topic, 1001, { Sessions: { Tree: { changed: ["index.html"] } } }), site);
  await expect(body).toHaveText("later");
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

test("image controls preserve focus, zoom and native image until the tab closes", async ({ page }) => {
  const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=";
  await openDesktop(page, true, { agent_tree: listing(["image.png"]), agent_file: { size: 68, mime: "image/png", data: png } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="image.png"]').click();
  const stage = page.locator(".agpstage.img"), image = stage.locator("img");
  await expect(image).toBeVisible();
  await expect(page.locator(".agpview:visible .zoom")).not.toHaveText("");
  await image.evaluate(element => { element.dataset.identity = "kept"; });
  const actual = page.getByRole("button", { name: "Actual size" });
  await actual.click();
  await expect(actual).toBeFocused();
  await expect(actual).toHaveClass(/on/);
  await stage.dispatchEvent("wheel", { deltaY: -40, ctrlKey: true, clientX: 100, clientY: 100 });
  await expect(page.locator(".agpview:visible .zoom")).not.toHaveText("100%");
  await expect(image).toHaveAttribute("data-identity", "kept");
  await expect(actual).toBeFocused();
  const bounds = await stage.boundingBox(); assertBounds(bounds);
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.down();
  await page.mouse.move(bounds.x + bounds.width / 2 + 15, bounds.y + bounds.height / 2 + 10);
  await page.mouse.up();
  await expect(image).toHaveAttribute("data-identity", "kept");
  await page.getByRole("button", { name: "Fit", exact: true }).click();
  await page.getByRole("button", { name: "Open in Preview" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_file_open").length)).toBe(1);
  await stage.dispatchEvent("wheel", { deltaY: -40, ctrlKey: true, clientX: 100, clientY: 100 });
  await page.mouse.move(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2);
  await page.mouse.down();
  await page.locator('[role="tab"][data-mtab] [data-mclose]').evaluate(element => { if (element instanceof HTMLElement) element.click(); });
  await page.mouse.up();
  await expect(stage).toHaveCount(0);
});

function assertBounds(value: { x: number; y: number; width: number; height: number } | null): asserts value is { x: number; y: number; width: number; height: number } {
  if (!value) throw new Error("Image stage has no bounds");
}

test("file loading progress updates the retained pending view", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["slow.png"]) });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_file") return invoke<T>(command, args);
      if (typeof args?.offset === "number" && args.offset > 0) await new Promise<void>(resolve => window.addEventListener(`release-image-part-${args.offset}`, () => resolve(), { once: true }));
      return { size: 3, mime: "image/png", data: "AA==" } as T;
    };
    window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } });
  });
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="slow.png"]').click();
  const pending = page.locator(".agpview:visible .agpmsg").filter({ hasText: "Opening slow.png" });
  await expect(pending).toBeVisible();
  await expect(pending.locator(".agpload i")).toHaveCSS("transform", "matrix(0.333333, 0, 0, 1, 0, 0)");
  await pending.evaluate(element => { element.dataset.identity = "kept"; });
  await page.evaluate(() => window.dispatchEvent(new Event("release-image-part-1")));
  await expect(pending.locator(".agpload i")).toHaveCSS("transform", "matrix(0.666667, 0, 0, 1, 0, 0)");
  await expect(pending).toHaveAttribute("data-identity", "kept");
  await page.evaluate(() => window.dispatchEvent(new Event("release-image-part-2")));
  await expect(page.locator('.agpstage.img img[alt="slow.png"]')).toBeVisible();
});

test("a late file chunk cannot remount a closed loading view", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["slow.png"]) });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_file") return invoke<T>(command, args);
      await new Promise<void>(resolve => window.addEventListener("release-closed-file", () => resolve(), { once: true }));
      Reflect.set(window, "closedFileDelivered", true);
      return { size: 1, mime: "image/png", data: "AA==" } as T;
    };
    window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } });
  });
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="slow.png"]').click();
  const host = page.locator(".agpview:visible").filter({ hasText: "Opening slow.png" });
  await expect(host).toBeVisible();
  await host.evaluate(element => Reflect.set(window, "closedFileHost", element));
  await page.locator('[role="tab"][data-mtab] [data-mclose]').click();
  await page.evaluate(() => window.dispatchEvent(new Event("release-closed-file")));
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "closedFileDelivered"))).toBe(true);
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  expect(await page.evaluate(() => (Reflect.get(window, "closedFileHost") as HTMLElement).isConnected)).toBe(false);
  await expect(page.locator(".agpview:not(.files)").filter({ hasText: "slow.png" })).toHaveCount(0);
  await expect(page.locator('.agpstage.img img[alt="slow.png"]')).toHaveCount(0);
});

test("files keep working after leaving the session and opening it again", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["a.txt", "b.txt"]) });
  const session = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example session", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;
  await page.evaluate(value => window.__navigation.open({ agent: value }), session);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const tree = page.getByRole("tree", { name: "Working files" });
  await expect(tree.locator('[data-fpath="a.txt"]')).toBeVisible();

  await page.evaluate(() => window.__navigation.open("home"));
  await page.evaluate(value => window.__navigation.open({ agent: value }), session);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await expect(tree.locator('[data-fpath="a.txt"]')).toBeVisible();

  await page.getByRole("textbox", { name: "Filter files" }).fill("b.t");
  await expect(tree.locator('[data-fpath="a.txt"]')).toHaveCount(0);
  await tree.locator('[data-fpath="b.txt"]').click({ modifiers: ["Meta"] });
  await expect(tree.locator('[data-fpath="b.txt"]')).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".agpacts .meta")).toHaveText("1 selected");
});

test("a tree read that answers nothing shows a plain message, not an exception", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: null });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  const tree = page.getByRole("tree", { name: "Working files" });
  await expect(tree).toContainText("The machine sent no file listing.");
  await expect(tree).not.toContainText("Cannot read properties");
});

test("a text file edits in the pane, saves with the shortcut and stops when it changed", async ({ page }) => {
  const modifier = process.platform === "darwin" ? "Meta" : "Control";
  await openDesktop(page, true, { agent_tree: listing(["notes.txt"]), agent_tree_text: { size: 6, text: "hello\n", cut: false, binary: false, editable: true }, agent_tree_edit: null });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="notes.txt"]').click();
  await page.locator("[data-tedit]").click();

  const editor = page.locator(".agcm .cm-content");
  await expect(editor).toBeFocused();
  await page.keyboard.press(`${modifier}+End`);
  await page.keyboard.type("world");
  await page.keyboard.press(`${modifier}+s`);
  const writes = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree_edit").map(call => call.args));
  await expect.poll(writes).toEqual([{ machineId: "build-box", root: "/srv/project", action: { Write: { path: "notes.txt", text: "hello\nworld" } } }]);
  await expect(page.locator("[data-tsave]")).toBeDisabled();

  // The agent changed the file since: the save stops and asks.
  await page.evaluate(() => window.__navigation.respond("agent_tree_text", { size: 3, text: "new", cut: false, binary: false, editable: true }));
  await page.keyboard.type("!");
  await page.keyboard.press(`${modifier}+s`);
  await expect(page.locator("[data-toverwrite]")).toBeVisible();
  expect(await writes()).toHaveLength(1);
  await page.locator("[data-treload]").click();
  await expect(editor).toHaveText("new");

  await page.keyboard.type("x");
  await page.locator("[data-tdone]").click();
  const dialog = page.getByRole("dialog").filter({ hasText: "Discard changes" });
  await dialog.getByRole("button", { name: "Discard" }).click();
  await expect(page.locator("[data-tedit]")).toBeVisible();
  expect(await writes()).toHaveLength(1);
});

test("a double click on a text file edits it from there, with a selection that shows on a light skin", async ({ page }) => {
  const modifier = process.platform === "darwin" ? "Meta" : "Control";
  await openDesktop(page, true, { agent_tree: listing(["notes.txt"]), agent_tree_text: { size: 18, text: "first\nlet value = 1\n", cut: false, binary: false, editable: true }, agent_tree_edit: null });
  await page.evaluate(() => { document.documentElement.dataset.skin = "warm"; });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await page.locator('[data-fpath="notes.txt"]').click();

  // The middle of "value", on the second line.
  const at = await page.locator(".agcode pre").evaluate(pre => {
    const walker = document.createTreeWalker(pre, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      const start = node.textContent?.indexOf("value") ?? -1;
      if (start < 0) continue;
      const range = document.createRange();
      range.setStart(node, start + 2); range.setEnd(node, start + 3);
      const box = range.getBoundingClientRect();
      return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
    }
    return null;
  });
  if (!at) throw new Error("The code view does not show the text.");
  await page.mouse.dblclick(at.x, at.y);

  const editor = page.locator(".agcm .cm-content");
  await expect(editor).toBeFocused();
  await page.keyboard.type("my");
  await page.keyboard.press(`${modifier}+s`);
  const writes = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree_edit").map(call => call.args));
  await expect.poll(writes).toEqual([{ machineId: "build-box", root: "/srv/project", action: { Write: { path: "notes.txt", text: "first\nlet myvalue = 1\n" } } }]);

  await page.keyboard.press(`${modifier}+a`);
  const selected = await page.locator(".agcm .cm-selectionBackground").first().evaluate(box => getComputedStyle(box).backgroundColor);
  expect(selected).not.toBe("rgb(34, 51, 51)");
});

test("a middle click closes a file tab and leaves fixed tabs open", async ({ page }) => {
  await openDesktop(page, true, { agent_tree: listing(["notes.txt"]), agent_tree_text: { size: 6, text: "hello\n", cut: false, binary: false, editable: true } });
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  const files = page.getByRole("tab", { name: "Files", exact: true });
  await files.click();
  await page.locator('[data-fpath="notes.txt"]').click();
  const file = page.locator('[role="tab"][data-mtab]').filter({ hasText: "notes.txt" });
  await expect(file).toBeVisible();

  await files.click({ button: "middle" });
  await expect(files).toBeVisible();

  await file.click({ button: "middle" });
  await expect(file).toHaveCount(0);
});
