import { expect, test, type Page } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const session = { kind: "agent" as const, machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Image detail", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false };
const path = "/srv/project/.beep/uploads/screenshot.png";
const tiny = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

test("an assistant can start with a pasted image and no project folder", async ({ page }) => {
  const cached = "/cache/beep-node/attachments/draft/files/screenshot.png";
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: session.machine_id, machine_name: session.machine_name, agents: ["codex"], home: "/srv", workspace: null, capabilities: [], assistant: true }],
    agent_sessions: [],
    assistant_settings_get: { agent: "codex", model: null, effort: null, fast: null },
    agent_begin: { session: { ...session, cwd: "/srv", assistant: true }, why: "" },
    agent_tree_upload: cached,
  });
  await page.locator("#side").getByRole("button", { name: "Assistant", exact: true }).click();
  const prompt = page.locator("#agprompt");
  await prompt.fill("Read the screenshot");
  await prompt.evaluate((field, png) => {
    const data = new DataTransfer();
    const bytes = Uint8Array.from(atob(png), char => char.charCodeAt(0));
    data.items.add(new File([bytes], "screenshot.png", { type: "image/png" }));
    field.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, tiny);
  await expect(page.getByRole("button", { name: "1 file", exact: true })).toBeVisible();
  await prompt.press("Enter");

  const calls = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_begin"));
  await expect.poll(calls).toMatchObject([{ args: {
    machineId: session.machine_id, agent: "codex", assistant: true, cwd: null, prompt: `Read the screenshot\n\n@${cached}`,
    images: [{ name: "screenshot.png", mime: "image/png", data: tiny }],
  } }]);
  const uploads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree_upload"));
  expect(uploads).toMatchObject([{ args: { machineId: session.machine_id, root: "", path: "screenshot.png", attachment: { agent: "draft", session: expect.any(String) } } }]);
});

async function screenshot(page: Page) {
  return page.evaluate(() => {
    const canvas = document.createElement("canvas");
    canvas.width = 1800;
    canvas.height = 1200;
    const cx = canvas.getContext("2d");
    if (!cx) throw new Error("Canvas unavailable");
    cx.fillStyle = "white";
    cx.fillRect(0, 0, canvas.width, canvas.height);
    cx.fillStyle = "black";
    cx.font = "16px monospace";
    for (let y = 25; y < 1200; y += 25) cx.fillText("Readable screenshot text, session 123, detailed status", 20, y);
    return canvas.toDataURL("image/png").split(",")[1];
  });
}

async function openSession(page: Page, entries: { kind: string; text: string }[] = [], responses: Record<string, unknown> = {}) {
  await openDesktop(page, false, {
    agent_view: { session, model: null, plan: [], earlier: 0, error: null, turns: entries.length, queued: 0, queued_messages: [], entries },
    agent_steer: [], agent_tree_upload: path, ...responses,
  });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
}

async function sentImages(page: Page) {
  return page.evaluate(() => {
    const call = window.__navigation.calls().filter(call => call.command === "agent_steer").pop();
    const steer = call?.args.steer;
    if (typeof steer !== "object" || steer === null || !("steer" in steer)) return [];
    const prompt = steer.steer;
    if (typeof prompt !== "object" || prompt === null || !("images" in prompt) || !Array.isArray(prompt.images)) return [];
    return prompt.images.map((image: unknown) => {
      if (typeof image !== "object" || image === null || !("data" in image) || typeof image.data !== "string" || !("mime" in image)) throw new Error("Invalid image");
      return { data: image.data, mime: image.mime };
    });
  });
}

for (const mode of ["original", "oversized", "aggregate", "separate batches"] as const) test(`send ${mode} images without unreadable previews`, async ({ page }) => {
  await openSession(page);
  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_tree_upload") {
        const name = String(args?.path).split("/").pop();
        window.__navigation.respond(command, `/srv/project/.beep/uploads/${name}`);
      }
      return invoke<T>(command, args);
    };
  });
  const data = await screenshot(page);
  const png = Buffer.from(data, "base64");
  const buffer = mode === "original" ? png : Buffer.concat([png, Buffer.alloc((mode === "oversized" ? 6 : 4) * 1024 * 1024)]);
  const count = mode === "aggregate" || mode === "separate batches" ? 2 : 1;
  const files = Array.from({ length: count }, (_, i) => ({ name: `screenshot-${i}.png`, mimeType: "image/png", buffer }));
  const picker = page.locator('#agfield input[type="file"]');
  if (mode === "separate batches") {
    await picker.setInputFiles([files[0]]);
    await page.locator("#agfield .attachment-count").click();
    await expect(page.locator(".attachment-gallery [data-agatt]")).toHaveCount(1);
    await page.getByRole("button", { name: "Close attachments" }).click();
    await expect(page.locator("#agfield .attachment-count")).toBeFocused();
    await picker.setInputFiles([files[1]]);
  } else await picker.setInputFiles(files);
  await expect(page.locator("#agfield .attachment-count")).toHaveText(`${count} ${count === 1 ? "file" : "files"}`);
  await page.locator("#agmsg").fill("Read the screenshot");
  await page.locator("#agmsg").press("Enter");
  await expect.poll(async () => (await sentImages(page)).length).toBe(count);
  const images = await sentImages(page);
  if (mode === "original") expect(images).toEqual([{ data, mime: "image/png" }]);
  const sizes = images.map(image => Buffer.from(image.data, "base64").length);
  expect(Math.max(...sizes)).toBeLessThanOrEqual(5 * 1024 * 1024);
  expect(sizes.reduce((a, b) => a + b, 0)).toBeLessThanOrEqual(7 * 1024 * 1024);
  for (const image of images) {
    const dimensions = await page.evaluate(async image => {
      const bitmap = await createImageBitmap(await (await fetch(`data:${image.mime};base64,${image.data}`)).blob());
      const dimensions = [bitmap.width, bitmap.height];
      bitmap.close();
      return dimensions;
    }, image);
    expect(dimensions).toEqual([1800, 1200]);
  }
  await page.locator(".dcoderen.user img.pic").first().click();
  await expect(page.locator("#viewer .vi img")).toHaveAttribute("src", `data:image/png;base64,${buffer.toString("base64")}`);
});

for (const count of [1, 2]) test(`image-only message sends a default request with ${count} originals`, async ({ page }) => {
  await openSession(page);
  await page.locator('#agfield input[type="file"]').setInputFiles(Array.from({ length: count }, (_, i) => ({
    name: `screenshot-${i}.png`, mimeType: "image/png", buffer: Buffer.from(tiny, "base64"),
  })));
  await expect(page.locator("#agfield .attachment-count")).toHaveText(`${count} ${count === 1 ? "file" : "files"}`);
  await page.locator("#agmsg").press("Enter");
  await expect.poll(async () => (await sentImages(page)).length).toBe(count);
  const sent = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_steer").at(-1)?.args);
  expect(sent).toMatchObject({ steer: { steer: { text: count === 1 ? "Check this image." : "Check these images." } } });
  await expect(page.locator(".dcoderen.user")).toContainText(count === 1 ? "Check this image." : "Check these images.");
});

test("saved prompts open original uploads instead of degraded transcript images", async ({ page }) => {
  await openSession(page, [{ kind: "user", text: `Read this.\n\n@"${path}"\n[image: image-0123456789abcdef]` }], {
    agent_thumb: `data:image/png;base64,${tiny}`, agent_image: `data:image/png;base64,${tiny}`,
  });
  const original = await screenshot(page);
  await page.evaluate(original => window.__navigation.respond("agent_file", { size: atob(original).length, mime: "image/png", data: original }), original);
  const tile = page.locator(".dcoderen.user img.pic");
  await expect(tile).toHaveAttribute("src", `data:image/png;base64,${tiny}`);
  await tile.click();
  await expect(page.locator("#viewer .vi img")).toHaveAttribute("src", `data:image/png;base64,${original}`);
  const reads = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_file").map(call => call.args.path));
  expect(reads).toEqual([path]);
});

for (const entry of ["link", "files", "preview"]) {
  test(`SVG opened through ${entry} renders as an image`, async ({ page }) => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><rect width="120" height="80" fill="red"/></svg>';
    const name = "diagram.SVG";
    const message = { kind: "agent", text: `[Diagram](${name})`, html: `<p><a href="${name}">Diagram</a></p>` };
    await openSession(page, [message], {
      agent_file: { size: Buffer.byteLength(svg), mime: "image/svg+xml", data: Buffer.from(svg).toString("base64") },
      agent_tree: { folders: [{ path: "", entries: [{ name, folder: false, size: svg.length, ignored: false, change: null }], more: 0 }], truncated: false },
    });
    if (entry === "link") {
      await page.getByRole("link", { name: "Diagram", exact: true }).click();
    } else {
      await page.getByRole("tab", { name: "Files", exact: true }).click();
      const row = page.locator(`[data-fpath="${name}"]`);
      if (entry === "preview") {
        await row.click({ button: "right" });
        await page.locator('[data-fact="preview"]').click();
      } else await row.dblclick();
    }
    const image = page.locator(".agpstage.img img");
    await expect(image).toBeVisible();
    await expect(image).toHaveAttribute("alt", name);
    await expect.poll(() => image.evaluate(element => element instanceof HTMLImageElement ? [element.naturalWidth, element.naturalHeight] : null)).toEqual([120, 80]);
    expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "agent_tree_text" || call.command === "agent_artifact"))).toEqual([]);
  });
}
