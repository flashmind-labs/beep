import { expect, test } from "@playwright/test";
import { openDesktop, projectName } from "./harness.ts";

const session = { kind: "agent" as const, machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Photo models", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false };
const paths = Array.from({ length: 28 }, (_, i) => `/path/to/uploads/batch-${i}/files/photo-${i}.jpg`);

for (const width of [1280, 390]) test(`large sent and pending batches stay compact at ${width}px`, async ({ page }, info) => {
  await page.setViewportSize({ width, height: 900 });
  const text = `Create models.\n\n${paths.map(path => `@${path}`).join(" ")}`;
  await openDesktop(page, false, {
    agent_view: { session, model: null, plan: [], earlier: 0, error: null, turns: 1, queued: 0, queued_messages: [], entries: [{ kind: "user", text, html: width === 390 ? null : `<p>Create models.</p><p>${paths.map(path => `@${path}`).join(" ")}</p>` }] },
  });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  const sent = page.locator(".dcoderen.user .attachment-batch");
  await expect(sent.locator("summary")).toHaveText("28 attachments");
  await expect(sent.locator(".attachment-file").first()).toBeHidden();
  await sent.locator("summary").click();
  await expect(sent.locator(".attachment-file")).toHaveCount(28);
  await expect(sent.locator(".attachment-file").first()).toHaveAttribute("title", paths[0]);
  expect((await sent.locator(".attachment-items").boundingBox())!.height).toBeLessThanOrEqual(152);
  await sent.locator("summary").click();

  await page.evaluate(() => {
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      if (command === "agent_tree_upload") window.__navigation.respond(command, `/path/to/uploads/batch/files/${String(args?.path)}`);
      return invoke<T>(command, args);
    };
  });
  const image = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64");
  await page.locator('#agfield input[type="file"]').setInputFiles(Array.from({ length: 28 }, (_, i) => ({ name: `photo-${i}.png`, mimeType: "image/png", buffer: image })));
  const count = page.locator("#agfield .attachment-count");
  await expect(count).toHaveText("28 files");
  expect((await count.boundingBox())!.height).toBeLessThan(44);
  await count.click();
  const attachments = page.getByRole("dialog", { name: "Attachments", exact: true });
  await expect(attachments).toBeVisible();
  await expect(attachments.locator(".attachment-gallery img")).toHaveCount(28);
  const last = attachments.getByRole("button", { name: "Preview photo-27.png", exact: true });
  await last.click();
  const viewer = page.locator("#viewer");
  await expect(viewer).toBeVisible();
  await expect(viewer.locator(".vi img")).toBeVisible();
  expect((await viewer.boundingBox())!.height).toBe(page.viewportSize()!.height);
  await viewer.getByRole("button", { name: "Close", exact: true }).click();
  await expect(viewer).toHaveCount(0);
  await expect(attachments).toBeVisible();
  await attachments.getByRole("button", { name: "Remove photo-27.png", exact: true }).click();
  await expect(attachments.locator(".attachment-gallery img")).toHaveCount(27);
  await page.screenshot({ path: info.outputPath(`attachments-${width}.png`) });
  await attachments.getByRole("button", { name: "Close attachments" }).click();
  await expect(count).toHaveText("27 files");
  await expect(count).toBeFocused();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("new conversation attachment window previews all local images", async ({ page }) => {
  await openDesktop(page, false, {
    agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: null, home: "/srv", capabilities: [] }],
  });
  await page.evaluate(project => window.__navigation.open({ projects: { project } }), projectName);
  const form = page.locator(".cc-pjc:visible");
  const image = Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=", "base64");
  await form.locator('input[type="file"]').setInputFiles(Array.from({ length: 28 }, (_, i) => ({ name: `photo-${i}.png`, mimeType: "image/png", buffer: image })));
  await form.getByRole("button", { name: "28 files", exact: true }).click();
  const attachments = page.getByRole("dialog", { name: "Attachments", exact: true });
  await expect(attachments.locator(".attachment-gallery img")).toHaveCount(28);
  await attachments.getByRole("button", { name: "Preview photo-27.png", exact: true }).click();
  await expect(page.locator("#viewer .vi img")).toBeVisible();
  await page.locator("#viewer").getByRole("button", { name: "Close", exact: true }).click();
  await attachments.getByRole("button", { name: "Remove photo-27.png", exact: true }).click();
  await expect(attachments.locator(".attachment-gallery img")).toHaveCount(27);
  await page.keyboard.press("Escape");
  await expect(attachments).toHaveCount(0);
  await expect(form.getByRole("button", { name: "27 files", exact: true })).toBeVisible();
});
