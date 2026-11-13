import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import type { Locator, Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

interface Box { x: number; y: number; width: number; height: number }

const art = (name: string) => fileURLToPath(new URL(`../../../../plugins/examples/composer-cat/art/${name}`, import.meta.url));
const docs = (name: string) => fileURLToPath(new URL(`../../../../docs/${name}`, import.meta.url));

/// A sprite from disk as a data URL, with its pixel size read from the PNG header.
function sprite(name: string) {
  const bytes = readFileSync(art(name));
  return { src: `data:image/png;base64,${bytes.toString("base64")}`, width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

function placement(name: string) {
  const { src, width, height } = sprite(name);
  return { plugin: "composer-cat", slot: "composer", align: "end", offset: 84, sink: 3, node: { kind: "image", src, width: width * 2, height: height * 2, pixelated: true } };
}

const agent = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false };
const commands = [{ name: "review", description: "Review work", hint: "[path]" }, { name: "run", description: "Run checks", hint: "" }];

async function openSession(page: Page, placements: unknown[]): Promise<void> {
  await openDesktop(page, true, { agent_commands: commands, plugins_placements: placements });
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

  const image = page.locator(".pluginslot img");
  await expect(image).toBeVisible();
  await expect.poll(() => image.evaluate((element: HTMLImageElement) => element.naturalWidth)).toBe(natural.width);

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
  await expect.poll(() => image.getAttribute("src")).toBe(blink.node.src);
  expect(blink.node.src).not.toBe(before);
  await page.evaluate(() => window.__navigation.emit("plugins-changed", []));
  await expect(page.locator(".pluginslot")).toHaveCount(0);
  expect(await box(page.locator("#agfield"))).toEqual(field);
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
