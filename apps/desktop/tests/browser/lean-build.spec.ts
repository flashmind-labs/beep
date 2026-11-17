import { expect, test } from "@playwright/test";
import { openDesktop, openStartForm } from "./harness.ts";
import type { BuildCapabilities } from "../../src/ui/tauri.ts";

const capabilities: BuildCapabilities = {
  local_host: false, metrics_recording: false, resource_profiling: false, autostart: false,
  window_state: false, image_clipboard: false, desktop_control: false, rich_text: false,
};
const machine = { id: "build-box", name: "build-box", online: true, can_host: true, seen_at: 0, version: "1", env: null, joined: true, apps: [], gpus: [], capabilities: [], agents: 0, stats: null };
const settings = { name: "Controller", version: "0.1", autostart: false, agent_notices: false, save_folder: null, save_folder_picked: false, developer: false };
const responses = {
  build_capabilities: capabilities, settings,
  mac_control: { id: "controller", name: "Controller", enabled: false, status: "connected", error: null, permissions: {} },
  list_machines: [machine],
  center_home: { machines: [machine], projects: [{ display_name: null, name: "example/widget", url: "https://example.test/example/widget", machine_id: "build-box", machine_name: "build-box", folder: "/srv/project", cloned: true, imported_at: 1 }], running: [], needs_you: [] },
  agent_hosts: [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex"], workspace: "/srv/project", home: "/srv", capabilities: [] }],
};

test("lean settings explain local work and omit unavailable switches", async ({ page }) => {
  await openDesktop(page, false, responses);
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await sheet.getByRole("button", { name: "General", exact: true }).click();
  await expect(sheet.getByText("Lean build", { exact: true })).toBeVisible();
  await expect(sheet.getByRole("switch", { name: "Launch at login" })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Choose…", exact: true })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Show…", exact: true })).toBeVisible();
  await sheet.getByRole("button", { name: "This computer", exact: true }).click();
  await expect(sheet.getByText("Local work is not available", { exact: true })).toBeVisible();
  await expect(sheet.getByRole("switch", { name: "Let Beep use this computer" })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Allow…" })).toHaveCount(0);
  expect(await sheet.ariaSnapshot()).toMatchSnapshot("lean-computer.txt");
  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "build_capabilities").length)).toBe(1);
});

test("lean controller pairs machines without offering to host locally", async ({ page }) => {
  await openDesktop(page, false, { ...responses, center_home: { machines: [], projects: [], running: [], needs_you: [] }, mesh_invite: { token: "example-ticket", command: "beep-node join example-ticket", qr_svg: null, expires_at: Date.now() + 60_000 }, ticket_machine: null });
  await page.getByRole("button", { name: "Add a machine", exact: true }).last().click();
  const sheet = page.getByRole("dialog", { name: "Add a machine" });
  await expect(sheet.getByText("beep-node join example-ticket", { exact: true })).toBeVisible();
  await expect(sheet.getByText("This computer", { exact: true })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: /Use this computer/ })).toHaveCount(0);
  await expect(sheet.getByRole("radio", { name: /^Desktop/ })).toBeVisible();
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "set_mac_control").length)).toBe(0);
});

test("lean controller keeps remote projects and agent starts", async ({ page }) => {
  await openDesktop(page, false, responses);
  await openStartForm(page);
  await expect(page.locator("#agprompt")).toBeVisible();
  await expect(page.locator("#agpillb")).toBeEnabled();
  await page.keyboard.press("Escape");
  await page.locator("#side").getByRole("button", { name: "Add a project", exact: true }).click();
  await page.getByRole("dialog", { name: "Add a project" }).getByRole("button", { name: "Clone repository", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Import a project" });
  await sheet.getByRole("textbox", { name: "Repository" }).fill("example/project");
  await expect(sheet.getByRole("button", { name: "build-box", exact: true })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Controller", exact: true })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Import", exact: true })).toBeEnabled();
});

test("lean image viewer keeps saving without image clipboard actions", async ({ page }) => {
  const tiny = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
  const session = { kind: "agent" as const, machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Image detail", cwd: "/srv/project", status: "idle", doing: null, started_at: 1, last_activity: 1, external: false };
  const source = '<img src="invalid" onerror="window.unsafeMarkup = true">';
  await openDesktop(page, false, { ...responses,
    agent_view: { session, model: null, plan: [], files: [], earlier: 0, last_message: "", error: null, turns: 2, queued: 0, queued_messages: [], entries: [
      { kind: "agent", text: source },
      { kind: "user", text: 'Read this.\n\n@"/srv/project/picture.png"\n[image: image-0123456789abcdef]' },
    ] }, agent_thumb: `data:image/png;base64,${tiny}`, agent_file: { size: 68, mime: "image/png", data: tiny },
  });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await expect(page.locator(".reply-body")).toHaveText(source);
  await expect(page.locator('img[src="invalid"]')).toHaveCount(0);
  expect(await page.evaluate(() => Reflect.get(window, "unsafeMarkup") ?? null)).toBe(null);
  await page.locator(".dcoderen.user img.pic").click();
  const viewer = page.locator("#viewer");
  await expect(viewer.getByRole("button", { name: /^Save/ })).toBeVisible();
  await expect(viewer.getByRole("button", { name: /^Copy/ })).toHaveCount(0);
  await page.keyboard.press("Meta+c");
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "copy_diagram_image").length)).toBe(0);
});


test("invalid capabilities keep local actions unavailable", async ({ page }) => {
  await openDesktop(page, false, { ...responses, build_capabilities: { local_host: true } });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await sheet.getByRole("button", { name: "This computer", exact: true }).click();
  await expect(sheet.getByText("Local work is not available", { exact: true })).toBeVisible();
  await expect(sheet.getByRole("switch", { name: "Let Beep use this computer" })).toHaveCount(0);
});
