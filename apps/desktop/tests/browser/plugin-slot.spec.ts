import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import type { Locator, Page } from "@playwright/test";
import { bundleFixture, openDesktop, sessionSettled } from "./harness.ts";

interface Box { x: number; y: number; width: number; height: number }

const art = (name: string) => fileURLToPath(new URL(`../../../../plugins/examples/composer-cat/art/${name}`, import.meta.url));
const docs = (name: string) => fileURLToPath(new URL(`../../../../docs/${name}`, import.meta.url));

/// A frame from disk: its hash (any hex name does for the mock desktop), its bytes and its pixel size read from the PNG header.
function sprite(name: string) {
  const bytes = readFileSync(art(name));
  const hash = createHash("sha256").update(bytes).digest("hex");
  return { asset: { hash, mime: "image/png" }, base64: bytes.toString("base64"), width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

/// What the mock desktop serves for `plugins_asset`: every frame of the cat, by hash.
const assets = Object.fromEntries(readdirSync(art("")).map(name => { const { asset, base64 } = sprite(name); return [asset.hash, base64]; }));

const edge = { kind: "edge", align: "end", offset: 84, sink: 3 };

function placement(name: string) {
  const { asset, width, height } = sprite(name);
  return { plugin: "composer-cat", slot: "composer", instance: "composer", position: edge, enter: "none", leave: "none", node: { kind: "image", asset, width: width * 2, height: height * 2, pixelated: true } };
}

type Repeat = { kind: "once" } | { kind: "every"; minMs: number; maxMs: number };

/// The cat as a sprite: the idle frame at rest (frame 0) and one motion to the blink frame (frame 1).
function catSprite(repeat: Repeat, ms = 5000) {
  const idle = sprite("cat-idle.png");
  return { plugin: "composer-cat", slot: "composer", instance: "composer", position: edge, enter: "none", leave: "none", node: { kind: "sprite", frames: [idle.asset, sprite("cat-blink.png").asset], rest: 0, width: idle.width * 2, height: idle.height * 2, pixelated: true, motions: [{ steps: [{ frame: 1, ms }], repeat }], reactions: {}, play: null } };
}

type Step = { frame: number; ms: number };

/// A cat with three frames (idle, blink, happy), the given motions, a reaction each for `happy` (frame 2 for 200 ms,
/// then frame 1 for 100 ms) and `blink` (frame 1 for 100 ms), and the reaction asked for now. It enters and leaves as told.
function reacting(play: { name: string; id: number } | null, motions: { steps: Step[]; repeat: { kind: "loop" } | { kind: "once" } }[] = [], transitions: { enter: string; leave: string } = { enter: "none", leave: "none" }) {
  const idle = sprite("cat-idle.png");
  const frames = [idle.asset, sprite("cat-blink.png").asset, sprite("cat-happy.png").asset];
  const reactions = { happy: [{ frame: 2, ms: 200 }, { frame: 1, ms: 100 }], blink: [{ frame: 1, ms: 100 }] };
  return { plugin: "composer-cat", slot: "composer", instance: "composer", position: edge, ...transitions, node: { kind: "sprite", frames, rest: 0, width: idle.width * 2, height: idle.height * 2, pixelated: true, motions, reactions, play } };
}

const typing = [{ steps: [{ frame: 1, ms: 100 }, { frame: 2, ms: 100 }], repeat: { kind: "loop" as const } }];

const agent = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false };
const commands = [{ name: "review", description: "Review work", hint: "[path]" }, { name: "run", description: "Run checks", hint: "" }];

async function openSession(page: Page, placements: unknown[]): Promise<void> {
  await openDesktop(page, true, { agent_commands: commands, plugins_placements: placements, plugins_assets: assets });
  await page.evaluate(session => window.__navigation.open({ agent: session }), agent);
  await expect(page.locator("#agfield")).toBeVisible();
}

async function box(target: Locator): Promise<Box> {
  const found = await target.boundingBox();
  if (found === null) throw new Error("element has no box");
  return found;
}

/// Boxes of the ancestors that clip their content, nearest first.
function clippingAncestors(image: Locator) {
  return image.evaluate(element => {
    const clips: { tag: string; overflow: string; box: { x: number; y: number; width: number; height: number } }[] = [];
    for (let node = element.parentElement; node !== null; node = node.parentElement) {
      const style = getComputedStyle(node);
      if (style.overflowX === "visible" && style.overflowY === "visible") continue;
      const { x, y, width, height } = node.getBoundingClientRect();
      clips.push({ tag: `${node.tagName.toLowerCase()}${node.id ? `#${node.id}` : ""}`, overflow: `${style.overflowX} ${style.overflowY}`, box: { x, y, width, height } });
    }
    return clips;
  });
}

test("a composer plugin placement stands on the message box without changing layout", async ({ page, context }) => {
  const cat = placement("cat-idle.png");
  const natural = sprite("cat-idle.png");
  await openSession(page, [cat]);
  await sessionSettled(page);

  const image = page.locator(".pluginslot img");
  await expect(image).toBeVisible();
  await expect.poll(() => image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBe(natural.width);
  expect(await image.getAttribute("src")).toMatch(/^blob:/);

  const field = await box(page.locator("#agfield"));
  const slot = await box(page.locator(".pluginslot"));
  const cell = await box(image);

  // 1. Placement: feet 3 px below the box's top edge, 84 px in from the slot's right edge.
  expect(cell.width).toBe(natural.width * 2);
  expect(cell.height).toBe(natural.height * 2);
  expect(Math.abs(cell.y + cell.height - (field.y + 3))).toBeLessThanOrEqual(0.5);
  expect(Math.abs(slot.x + slot.width - 84 - (cell.x + cell.width))).toBeLessThanOrEqual(0.5);

  // 2. The composer does not move compared with a page that has no placement.
  const bare = await context.newPage();
  await openSession(bare, []);
  await sessionSettled(bare);
  await expect(bare.locator(".pluginslot")).toHaveCount(0);
  expect(await box(bare.locator("#agfield"))).toEqual(field);

  // 3. The sprite takes no pointer.
  const hit = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest(".pluginslot") !== null, { x: cell.x + cell.width / 2, y: cell.y + cell.height / 2 });
  expect(hit).toBe(false);

  // 4. Nothing clips it: it is inside the viewport and inside every clipping ancestor.
  const viewport = page.viewportSize();
  if (viewport === null) throw new Error("no viewport");
  expect(cell.x).toBeGreaterThanOrEqual(0);
  expect(cell.y).toBeGreaterThanOrEqual(0);
  expect(cell.x + cell.width).toBeLessThanOrEqual(viewport.width);
  expect(cell.y + cell.height).toBeLessThanOrEqual(viewport.height);
  for (const clip of await clippingAncestors(image)) {
    const label = `${clip.tag} (${clip.overflow})`;
    expect(cell.x, label).toBeGreaterThanOrEqual(clip.box.x);
    expect(cell.y, label).toBeGreaterThanOrEqual(clip.box.y);
    expect(cell.x + cell.width, label).toBeLessThanOrEqual(clip.box.x + clip.box.width);
    expect(cell.y + cell.height, label).toBeLessThanOrEqual(clip.box.y + clip.box.height);
  }

  // 6. The command menu opens in the same place with and without the placement.
  await page.locator("#agmsg").fill("/");
  await bare.locator("#agmsg").fill("/");
  await expect(page.locator(".cm-menu").getByRole("option")).toHaveCount(2);
  await expect(bare.locator(".cm-menu").getByRole("option")).toHaveCount(2);
  expect(await box(page.locator(".cm-menu"))).toEqual(await box(bare.locator(".cm-menu")));
  await bare.close();

  // 5. A change swaps the image; an empty list removes the slot.
  const blink = placement("cat-blink.png");
  const before = await image.getAttribute("src");
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [blink]);
  await expect.poll(async () => { const now = await image.getAttribute("src"); return now !== null && now.startsWith("blob:") && now !== before; }).toBe(true);
  await page.evaluate(() => window.__navigation.emit("plugins-changed", []));
  await expect(page.locator(".pluginslot")).toHaveCount(0);
  expect(await box(page.locator("#agfield"))).toEqual(field);
});

interface Published { topic: { name: string; key: string | null; machine: string | null }; machine: string | null; data: unknown }

/// What the page handed to `plugins_publish` so far.
async function published(page: Page): Promise<Published[]> {
  const calls = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "plugins_publish").map(call => call.args.update));
  return calls.filter((update): update is Published => typeof update === "object" && update !== null && "topic" in update && "data" in update);
}

test("opening a session publishes ui.composer to plugins", async ({ page }) => {
  await openSession(page, []);

  await expect.poll(async () => (await published(page)).map(update => [update.topic.name, update.data])).toContainEqual(["ui.composer", { machine: "build-box", session: "notice-one" }]);
});

test("the page does not carry mesh topics plugins want back to the host", async ({ page }) => {
  const wanted = [{ name: "agent", key: "notice-one", machine: "build-box" }, { name: "ui.composer", key: null, machine: null }];
  await openDesktop(page, true, { plugins_wanted: wanted });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "plugins_wanted"))).toBe(true);

  await page.evaluate(() => window.__navigation.publish("agent.notice-one", null));
  await page.evaluate(() => new Promise(resolve => setTimeout(resolve, 100)));

  expect((await published(page)).filter(update => update.topic.name === "agent")).toEqual([]);
});

interface Reported { reset: boolean; shown: { slot: string; key: string; context: Record<string, string> }[]; gone: { slot: string; key: string }[] }

/// The batches the page handed to `plugins_instances` so far.
async function batches(page: Page): Promise<Reported[]> {
  const calls = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "plugins_instances").map(call => call.args.batch));
  return calls.filter((batch): batch is Reported => typeof batch === "object" && batch !== null && "shown" in batch && "gone" in batch && "reset" in batch);
}

/// The events the page handed to `plugins_event` so far.
function events(page: Page): Promise<unknown[]> {
  return page.evaluate(() => window.__navigation.calls().filter(call => call.command === "plugins_event").map(call => call.args.event));
}

const inline = (slot: string, instance: string, node: unknown) => ({ plugin: "session-notes", slot, instance, position: { kind: "inline" }, enter: "none", leave: "none", node });
const note = (text: string) => ({ kind: "text", text, tone: "accent" });
const listed = [0, 1].map(index => ({ id: `agent-${index}`, machine_id: "build-box", machine_name: "Build box", agent: "codex", title: `Agent ${index}`, cwd: "/srv/project", status: "working", doing: "Working", started_at: index, last_activity: index, external: false, question: null }));

test("each session row reports its instance and draws its own plugin output", async ({ page }) => {
  await openDesktop(page, false, { agent_sessions: listed, session_moves: [] });
  await page.goto("https://app.test/index.html");
  const script = await bundleFixture("tests/browser/agents-bar-fixture.ts");
  await page.addScriptTag({ content: script });
  await page.evaluate(() => Reflect.get(window, "openTestAgents")());
  const rows = page.getByRole("option");
  await expect(rows).toHaveCount(2);
  const before = await box(rows.first());

  // The page tells the host both rows, with what each row shows, in one batch.
  await expect.poll(async () => (await batches(page)).flatMap(batch => batch.shown).filter(item => item.slot === "session.row").map(item => [item.key, item.context])).toEqual([
    ["build-box:agent-0", { machine: "build-box", session: "agent-0", title: "Agent 0" }],
    ["build-box:agent-1", { machine: "build-box", session: "agent-1", title: "Agent 1" }],
  ]);
  expect((await batches(page)).filter(batch => batch.shown.some(item => item.slot === "session.row"))).toHaveLength(1);

  // Each row draws the output for its own instance, and nothing moves when a row has none.
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [inline("session.row", "build-box:agent-1", note("second note"))]);
  await expect(rows.nth(1).locator(".pluginhere")).toHaveText("second note");
  await expect(rows.nth(0).locator(".pluginhere")).toHaveCount(0);
  expect(await box(rows.first())).toEqual(before);

  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [inline("session.row", "build-box:agent-0", note("first note")), inline("session.row", "build-box:agent-1", note("second note"))]);
  await expect(rows.nth(0).locator(".pluginhere")).toHaveText("first note");
  await expect(rows.nth(1).locator(".pluginhere")).toHaveText("second note");

  // A row that leaves the page reports as gone.
  await page.evaluate(async rest => { window.__navigation.respond("agent_sessions", rest); await Reflect.get(window, "refreshTestAgents")(); }, listed.slice(0, 1));
  await expect(rows).toHaveCount(1);
  await expect.poll(async () => (await batches(page)).flatMap(batch => batch.gone).filter(item => item.slot === "session.row")).toEqual([{ slot: "session.row", key: "build-box:agent-1" }]);
});

test("the open session's header reports its instance, and a header button press reaches plugins_event", async ({ page }) => {
  await openSession(page, []);
  await sessionSettled(page);

  await expect.poll(async () => (await batches(page)).flatMap(batch => batch.shown).filter(item => item.slot === "session.header")).toEqual([{ slot: "session.header", key: "build-box:notice-one", context: { machine: "build-box", session: "notice-one" } }]);
  const composer = (await batches(page)).flatMap(batch => batch.shown).filter(item => item.slot === "composer");
  expect(composer).toEqual([{ slot: "composer", key: "composer", context: { machine: "build-box", session: "notice-one" } }]);
  await expect(page.locator(".pluginhere")).toHaveCount(0);
  const head = await box(page.locator(".ag-head"));

  const controls = { kind: "row", gap: 6, children: [
    { kind: "input", label: "Session note", value: "", placeholder: "Add a note", onChange: 3, onSubmit: 4 },
    { kind: "button", label: "Save note", children: [{ kind: "text", text: "Save", tone: "normal" }], onPress: 5 },
    { kind: "toggle", label: "Pinned", value: false, onChange: 6 },
  ] };
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [inline("session.header", "build-box:notice-one", controls)]);

  const button = page.getByRole("button", { name: "Save note" });
  await expect(button).toBeVisible();
  expect((await box(page.locator(".ag-head"))).height).toBe(head.height);

  // In a narrower window the title keeps room to read and nothing in the header overlaps. In a narrow one the plugin leaves.
  const chrome = await page.locator(".ag-head").evaluate(element => window.innerWidth - element.clientWidth);
  await page.setViewportSize({ width: 800 + chrome, height: 900 });
  const parts = await Promise.all([".ag-head .tt", ".pluginhere"].map(selector => box(page.locator(selector))));
  expect(parts[0].width).toBeGreaterThan(100);
  for (const [i, a] of parts.entries()) for (const b of parts.slice(i + 1)) expect(a.x + a.width <= b.x + 0.5 || b.x + b.width <= a.x + 0.5).toBe(true);
  if (process.env.PLUGIN_SLOT_SHOTS === "1") await page.screenshot({ path: "../../docs/plugin-header-narrow.png", clip: { x: 0, y: 0, width: 1000, height: 120 } });
  await page.setViewportSize({ width: 650 + chrome, height: 900 });
  await expect(page.locator(".pluginhere")).toBeHidden();
  expect((await box(page.locator(".ag-head .tt"))).width).toBeGreaterThan(100);
  await page.setViewportSize({ width: 1280, height: 900 });
  await button.click();
  await expect.poll(() => events(page)).toEqual([{ plugin: "session-notes", id: 5, action: { kind: "press" } }]);

  // The toggle sends the opposite of what it shows. A keyboard press works as a click.
  const toggle = page.getByRole("switch", { name: "Pinned" });
  await toggle.focus();
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await events(page)).at(-1)).toEqual({ plugin: "session-notes", id: 6, action: { kind: "toggle", value: true } });

  // The input sends each change and, on Enter, the text for onSubmit. Typing stays out of the app's keys.
  const field = page.getByRole("textbox", { name: "Session note" });
  await field.click();
  await page.keyboard.type("hi");
  await expect(field).toHaveValue("hi");
  await page.keyboard.press("Enter");
  await expect.poll(async () => (await events(page)).slice(2)).toEqual([
    { plugin: "session-notes", id: 3, action: { kind: "text", value: "h" } },
    { plugin: "session-notes", id: 3, action: { kind: "text", value: "hi" } },
    { plugin: "session-notes", id: 4, action: { kind: "text", value: "hi" } },
  ]);
  await expect(page.locator("#agfield")).toBeVisible();

  // The plugin's value replaces what was typed.
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [inline("session.header", "build-box:notice-one", { ...controls, children: [{ ...controls.children[0], value: "" }, controls.children[1], controls.children[2]] })]);
  await expect(field).toHaveValue("hi");
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [inline("session.header", "build-box:notice-one", { ...controls, children: [{ ...controls.children[0], value: "cleared" }, controls.children[1], controls.children[2]] })]);
  await expect(field).toHaveValue("cleared");
});

test("a late echo of older text does not replace what the user typed since", async ({ page }) => {
  await openSession(page, []);
  const withValue = (value: string) => [inline("session.header", "build-box:notice-one", { kind: "input", label: "Session note", value, placeholder: "", onChange: 3, onSubmit: null })];
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), withValue(""));
  const field = page.getByRole("textbox", { name: "Session note" });
  await field.click();
  await page.keyboard.type("abc");

  // The plugin answers "a" and "ab" late, then "abc".
  for (const echo of ["a", "ab"]) {
    await page.evaluate(list => window.__navigation.emit("plugins-changed", list), withValue(echo));
    await expect(field).toHaveValue("abc");
  }
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), withValue("abc"));
  await expect(field).toHaveValue("abc");

  // A value the plugin sets on its own still shows.
  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), withValue(""));
  await expect(field).toHaveValue("");
});

test("a composer slot with a control takes the pointer there only, and the cat still takes none", async ({ page }) => {
  const cat = placement("cat-idle.png");
  const press = { plugin: "composer-cat", slot: "composer", instance: "composer", position: { kind: "edge", align: "start", offset: 8, sink: 0 }, enter: "none", leave: "none", node: { kind: "button", label: "Poke", children: [], onPress: 2 } };
  await openSession(page, [cat, press]);

  const slot = page.locator(".pluginslot");
  await expect(slot.getByRole("button", { name: "Poke" })).toBeVisible();
  await expect(slot).not.toHaveAttribute("aria-hidden", "true");
  const image = await box(page.locator(".pluginslot img"));
  const hit = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest(".pluginslot") !== null, { x: image.x + image.width / 2, y: image.y + image.height / 2 });
  expect(hit).toBe(false);

  await slot.getByRole("button", { name: "Poke" }).click();
  await expect.poll(() => events(page)).toEqual([{ plugin: "composer-cat", id: 2, action: { kind: "press" } }]);

  await page.evaluate(list => window.__navigation.emit("plugins-changed", list), [cat]);
  await expect(slot).toHaveAttribute("aria-hidden", "true");
});

/// The calls to plugin commands that carry placements, events and instances, which a sprite must not add to.
const traffic = (page: Page) => page.evaluate(() => window.__navigation.calls().filter(call => call.command.startsWith("plugins_") && call.command !== "plugins_asset" && call.command !== "plugins_visible").map(call => call.command));

/// Which frames of the sprite are visible, by index.
const visibleFrames = (page: Page) => page.locator(".pluginsprite img[data-on]").evaluateAll(images => images.map(image => Number(image.getAttribute("data-frame"))));

test("a sprite stacks its frames, shows the rest frame and takes no pointer", async ({ page }) => {
  await openSession(page, [catSprite({ kind: "every", minMs: 3_600_000, maxMs: 3_600_000 })]);

  const frames = page.locator(".pluginsprite img");
  await expect(frames).toHaveCount(2);
  await expect.poll(() => visibleFrames(page)).toEqual([0]);
  const natural = sprite("cat-idle.png");
  const cell = await box(page.locator(".pluginsprite"));
  expect([cell.width, cell.height]).toEqual([natural.width * 2, natural.height * 2]);
  await expect(frames.first()).toBeVisible();
  await expect(frames.nth(1)).toBeHidden();
  await expect.poll(() => frames.first().evaluate((element: HTMLImageElement) => element.naturalWidth)).toBe(natural.width);

  const hit = await page.evaluate(({ x, y }) => document.elementFromPoint(x, y)?.closest(".pluginslot") !== null, { x: cell.x + cell.width / 2, y: cell.y + cell.height / 2 });
  expect(hit).toBe(false);
});

/// Freezes the page's clock before it loads, so a sprite's timers fire only when the test runs the clock.
async function freezeClock(page: Page): Promise<void> {
  await page.clock.install({ time: 0 });
  await page.clock.pauseAt(1000);
}

test("a repeating motion changes the visible frame with no plugin traffic", async ({ page }) => {
  await freezeClock(page);
  await openSession(page, [catSprite({ kind: "every", minMs: 100, maxMs: 100 }, 300)]);
  await expect.poll(() => visibleFrames(page)).toEqual([0]);
  const before = await traffic(page);

  // The page plays the motion by itself, then rests, then plays it again. Each step is
  // exact: nothing moves until the clock runs.
  await page.clock.runFor(99);
  expect(await visibleFrames(page)).toEqual([0]);
  await page.clock.runFor(1);
  expect(await visibleFrames(page)).toEqual([1]);
  await page.clock.runFor(299);
  expect(await visibleFrames(page)).toEqual([1]);
  await page.clock.runFor(1);
  expect(await visibleFrames(page)).toEqual([0]);
  await page.clock.runFor(100);
  expect(await visibleFrames(page)).toEqual([1]);

  expect(await traffic(page)).toEqual(before);
  expect(before.filter(command => command === "plugins_event")).toEqual([]);
});

test("a motion that plays once starts at once and holds the rest frame after it", async ({ page }) => {
  await freezeClock(page);
  await openSession(page, [catSprite({ kind: "once" }, 200)]);

  await expect.poll(() => visibleFrames(page)).toEqual([1]);
  await page.clock.runFor(199);
  expect(await visibleFrames(page)).toEqual([1]);
  await page.clock.runFor(1);
  expect(await visibleFrames(page)).toEqual([0]);
  await page.clock.runFor(10_000);
  expect(await visibleFrames(page)).toEqual([0]);
});

test("reduced motion holds the rest frame", async ({ page }) => {
  await freezeClock(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  // A motion that plays at once would show frame 1 as soon as the sprite is drawn.
  await openSession(page, [catSprite({ kind: "once" })]);

  await expect(page.locator(".pluginsprite img")).toHaveCount(2);
  expect(await visibleFrames(page)).toEqual([0]);

  // Allowing motion again starts it.
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await expect.poll(() => visibleFrames(page)).toEqual([1]);
});

test("nothing animates while the window is hidden, and the desktop is told", async ({ page }) => {
  await freezeClock(page);
  await openSession(page, [catSprite({ kind: "once" })]);
  await expect.poll(() => visibleFrames(page)).toEqual([1]);

  await page.evaluate(() => window.__navigation.emit("center-hidden", null));
  expect(await visibleFrames(page)).toEqual([0]);
  const told = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "plugins_visible").map(call => call.args.visible));
  await expect.poll(told).toEqual([true, false]);

  await page.evaluate(() => window.__navigation.emit("center-shown", null));
  await expect.poll(() => visibleFrames(page)).toEqual([1]);
  await expect.poll(told).toEqual([true, false, true]);
});

test("an image used twice is requested once, and its URL is revoked when no placement uses it", async ({ page }) => {
  await page.addInitScript(() => {
    const revoked: string[] = [];
    const revoke = URL.revokeObjectURL.bind(URL);
    URL.revokeObjectURL = (url: string) => { revoked.push(url); revoke(url); };
    Object.defineProperty(window, "revokedUrls", { value: revoked });
  });
  const idle = placement("cat-idle.png");
  // The sprite's rest frame is the same image as the placement before it.
  await openSession(page, [idle, catSprite({ kind: "every", minMs: 3_600_000, maxMs: 3_600_000 })]);
  await expect(page.locator(".pluginsprite img")).toHaveCount(2);

  const asked = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "plugins_asset").map(call => String(call.args.hash)));
  const hashes = await asked();
  expect(hashes.sort()).toEqual([sprite("cat-blink.png").asset.hash, sprite("cat-idle.png").asset.hash].sort());

  await page.evaluate(() => window.__navigation.emit("plugins-changed", []));
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "revokedUrls").length)).toBe(2);
  expect(await asked()).toHaveLength(2);
});

for (const scheme of ["dark", "light"] as const) {
  test(`screenshot of the slot in the ${scheme} theme`, async ({ page }, info) => {
    // Pictures for review, written to the untracked docs folder only when asked.
    test.skip(process.env.PLUGIN_SLOT_SHOTS !== "1" || info.project.name !== "macOS", "set PLUGIN_SLOT_SHOTS=1 for the pictures");
    await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
    await openSession(page, [placement("cat-idle.png")]);
    await expect(page.locator(".pluginslot img")).toBeVisible();
    const field = await box(page.locator("#agfield"));
    const top = Math.max(0, field.y - 150);
    await page.screenshot({ path: docs(scheme === "dark" ? "plugin-slot.png" : "plugin-slot-alt.png"), clip: { x: 0, y: top, width: 1280, height: Math.min(900, field.y + field.height + 16) - top } });
  });
}

/// Sends the host's placements to the page, as `plugins-changed` does.
const change = (page: Page, placements: unknown[]) => page.evaluate(list => window.__navigation.emit("plugins-changed", list), placements);

test("a sprite plays a reaction once for each new id, then goes back to its motions", async ({ page }) => {
  await freezeClock(page);
  // A request that is there when the sprite appears is old news.
  await openSession(page, [reacting({ name: "blink", id: 7 }, typing)]);
  await expect(page.locator(".pluginsprite img")).toHaveCount(3);
  await expect.poll(() => visibleFrames(page)).toEqual([1]);
  const before = await traffic(page);

  // A new id plays the reaction over the typing loop, once.
  await change(page, [reacting({ name: "happy", id: 8 }, typing)]);
  await expect.poll(() => visibleFrames(page)).toEqual([2]);
  await page.clock.runFor(199);
  expect(await visibleFrames(page)).toEqual([2]);
  await page.clock.runFor(1);
  expect(await visibleFrames(page)).toEqual([1]);
  await page.clock.runFor(100);

  // The loop is back. The same id on a later draw does not play it again.
  expect(await visibleFrames(page)).toEqual([1]);
  await change(page, [reacting({ name: "happy", id: 8 }, typing)]);
  await page.clock.runFor(100);
  expect(await visibleFrames(page)).toEqual([2]);
  await page.clock.runFor(100);
  expect(await visibleFrames(page)).toEqual([1]);

  // A newer id plays again, even for the same reaction, and a change of motions in the same draw is kept.
  await change(page, [reacting({ name: "happy", id: 9 }, [])]);
  await expect.poll(() => visibleFrames(page)).toEqual([2]);
  await page.clock.runFor(300);
  expect(await visibleFrames(page)).toEqual([0]);
  await page.clock.runFor(10_000);
  expect(await visibleFrames(page)).toEqual([0]);

  expect((await traffic(page)).filter(command => command === "plugins_event")).toEqual(before.filter(command => command === "plugins_event"));
});

test("a looping motion pauses while the window is hidden, and a reaction does not play then", async ({ page }) => {
  await freezeClock(page);
  await openSession(page, [reacting(null, typing)]);
  await expect.poll(() => visibleFrames(page)).toEqual([1]);

  await page.clock.runFor(100);
  expect(await visibleFrames(page)).toEqual([2]);

  await page.evaluate(() => window.__navigation.emit("center-hidden", null));
  expect(await visibleFrames(page)).toEqual([0]);
  await page.clock.runFor(5000);
  expect(await visibleFrames(page)).toEqual([0]);
  await change(page, [reacting({ name: "happy", id: 1 }, typing)]);
  await page.clock.runFor(5000);
  expect(await visibleFrames(page)).toEqual([0]);

  await page.evaluate(() => window.__navigation.emit("center-shown", null));
  await expect.poll(() => visibleFrames(page)).toEqual([1]);
});

test("reduced motion holds the rest frame for a loop and a reaction", async ({ page }) => {
  await freezeClock(page);
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openSession(page, [reacting(null, typing)]);
  await expect(page.locator(".pluginsprite img")).toHaveCount(3);

  await change(page, [reacting({ name: "happy", id: 1 }, typing)]);
  await page.clock.runFor(1000);

  expect(await visibleFrames(page)).toEqual([0]);
});

const motion = (page: Page) => page.locator(".pluginmotion");

test("a placement that declares an enter is wrapped for it, and one that does not is not", async ({ page }) => {
  await openSession(page, [reacting(null, [], { enter: "rise", leave: "fade" })]);

  await expect(motion(page)).toHaveCount(1);
  await expect(motion(page)).toHaveAttribute("data-enter", "rise");
  await expect(motion(page)).toHaveAttribute("data-leave", "fade");
  await expect(motion(page)).not.toHaveAttribute("data-leaving");
  await expect(motion(page).locator(".pluginsprite")).toHaveCount(1);

  await change(page, [reacting(null, [])]);
  await expect(page.locator(".pluginsprite")).toHaveCount(1);
  await expect(motion(page)).toHaveCount(0);
});

test("a left placement stays for its leave, then goes", async ({ page }) => {
  await freezeClock(page);
  await openSession(page, [reacting(null, [], { enter: "none", leave: "rise" })]);
  await expect(page.locator(".pluginsprite")).toHaveCount(1);

  await change(page, []);
  await expect(motion(page)).toHaveAttribute("data-leaving", "");
  await page.clock.runFor(149);
  await expect(motion(page)).toHaveCount(1);
  await page.clock.runFor(1);
  await expect(page.locator(".pluginslot")).toHaveCount(0);

  // A placement that comes back before the leave is done stays.
  await change(page, [reacting(null, [], { enter: "none", leave: "rise" })]);
  await expect(page.locator(".pluginsprite")).toHaveCount(1);
  await change(page, []);
  await page.clock.runFor(100);
  await change(page, [reacting(null, [], { enter: "none", leave: "rise" })]);
  await page.clock.runFor(1000);
  await expect(page.locator(".pluginsprite")).toHaveCount(1);
  await expect(motion(page)).not.toHaveAttribute("data-leaving");
});

test("a placement with no leave goes at once, and so does one under reduced motion", async ({ page }) => {
  await freezeClock(page);
  await openSession(page, [reacting(null, [], { enter: "fade", leave: "none" })]);
  await change(page, []);
  await expect(page.locator(".pluginslot")).toHaveCount(0);

  await page.emulateMedia({ reducedMotion: "reduce" });
  await change(page, [reacting(null, [], { enter: "fade", leave: "fade" })]);
  await expect(page.locator(".pluginsprite")).toHaveCount(1);
  await change(page, []);
  await expect(page.locator(".pluginslot")).toHaveCount(0);
});

test("enter and leave animate only when the user allows motion", async ({ page }) => {
  await openSession(page, [reacting(null, [], { enter: "rise", leave: "rise" })]);
  const animation = () => motion(page).evaluate(element => getComputedStyle(element).animationName);

  expect(await animation()).toBe("plugin-rise-in");

  await page.emulateMedia({ reducedMotion: "reduce" });
  expect(await animation()).toBe("none");
});

test("the open session's turns reach plugins as session.turn, once for each change", async ({ page }) => {
  const wanted = { name: "session.turn", key: "notice-one", machine: "build-box" };
  await openDesktop(page, true, { agent_sessions: [], agent_commands: commands, plugins_wanted: [wanted] });
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "agent_follow" || args?.session !== "notice-one") return invoke<T>(command, args);
      const channel = args.onEvent as { onmessage(value: unknown): void };
      const base = await invoke<{ session: Record<string, unknown> }>("agent_view", { session: "notice-one" });
      Reflect.set(window, "pushTurn", (status: string, turns: number, last_stop: string | null) => {
        channel.onmessage({ kind: "snapshot", resumed: false, cursor: { start: 0, next: 0, fingerprint: `${status}${turns}` }, page: { ...base, entries: [], earlier: 0, turns, last_stop, session: { ...base.session, status } }, modes: null, tasks: [] });
      });
      return new Promise<T>(() => {});
    };
  });
  await page.evaluate(session => window.__navigation.open({ agent: session }), agent);
  await expect.poll(() => page.evaluate(() => Reflect.has(window, "pushTurn"))).toBe(true);
  const push = (status: string, turns: number, stop: string | null) => page.evaluate(([a, b, c]) => (Reflect.get(window, "pushTurn") as (...rest: unknown[]) => void)(a, b, c), [status, turns, stop] as const);

  await push("working", 0, null);
  await push("working", 0, null);
  await push("idle", 1, "EndTurn");
  await push("idle", 1, "EndTurn");
  await push("asking", 1, "EndTurn");
  await push("idle", 2, "MaxTokens");

  const turns = async () => (await published(page)).filter(update => update.topic.name === "session.turn");
  await expect.poll(async () => (await turns()).length).toBe(4);
  expect((await turns()).map(update => [update.topic, update.machine, update.data])).toEqual([
    [wanted, "build-box", { session: "notice-one", state: "working", outcome: null, turn: 0 }],
    [wanted, "build-box", { session: "notice-one", state: "idle", outcome: "finished", turn: 1 }],
    [wanted, "build-box", { session: "notice-one", state: "asking", outcome: "finished", turn: 1 }],
    [wanted, "build-box", { session: "notice-one", state: "idle", outcome: "token-limit", turn: 2 }],
  ]);
});
