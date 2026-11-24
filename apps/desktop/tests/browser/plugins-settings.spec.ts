import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { openDesktop, pageErrors } from "./harness.ts";

const docs = (name: string) => fileURLToPath(new URL(`../../../../docs/${name}`, import.meta.url));

const commit = "4f0915a8b3c2d1e0f9a8b7c6d5e4f30211223344";
const git = (name: string, extra: Record<string, unknown> = {}) => ({ kind: "git", url: `https://github.com/owner/${name}.git`, ref: null, subfolder: null, commit, ...extra });
const status = (id: string, name: string, extra: Record<string, unknown> = {}) => ({
  id, name, version: "0.2.0", description: `${name} does one small thing.`, permissions: ["ui.add"], approved: ["ui.add"],
  source: git(id), enabled: true, state: "running", ...extra,
});

/// Six plugins in six states.
const plugins = [
  status("composer-cat", "Composer cat", { description: "A cat sits on the message box." }),
  status("session-notes", "Session notes", { version: "0.1.0", description: "A note on each session's row, set from the session header.", permissions: ["ui.add", "sessions.read"], approved: ["ui.add", "sessions.read"], source: { kind: "local" }, state: "waiting" }),
  status("idle-timer", "Idle timer", { version: "1.0.3", description: "Shows how long a session has been quiet.", source: git("beep-plugins", { subfolder: "plugins/idle-timer", commit: "91c2e07a8b3c2d1e0f9a8b7c6d5e4f3021122334" }), enabled: false, state: "disabled" }),
  status("quote-board", "Quote board", { version: "0.4.0", description: "Adds a button that asks for sessions it was not allowed to read.", permissions: ["ui.add", "sessions.read"], approved: ["ui.add"], enabled: true, state: "needs-approval", missing: ["sessions.read"] }),
  status("dev-sketch", "Dev sketch", { version: "0.0.1", description: "Loaded from the plugins folder you set for development.", source: { kind: "dev" }, state: "stopped", reason: "used 612 ms of script time in a minute" }),
  status("tidy-header", "Tidy header", { version: "1.0.0", description: "Ships with Beep.", source: { kind: "builtin" } }),
];

const settings = { name: "build-box", version: "0.1", autostart: false, agent_notices: false };

interface FakeOptions { plugins: unknown[]; failRemove: boolean }

/// Stands in for the desktop's plugin commands inside the page: a list that changes as the page asks, and the same `{ kind, message }` errors.
function installFake(options: FakeOptions): void {
  const listed: unknown[] = structuredClone(options.plugins);
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  const core = window.__TAURI__.core;
  const original: (command: string, args?: Record<string, unknown>) => Promise<unknown> = core.invoke.bind(core);
  const find = (id: unknown) => listed.find((row): row is Record<string, unknown> => typeof row === "object" && row !== null && "id" in row && row.id === id);
  const source = (name: string) => ({ kind: "git", url: `https://github.com/owner/${name}.git`, ref: null, subfolder: null, commit: "4f0915a8b3c2d1e0f9a8b7c6d5e4f30211223344" });
  const preview = (kind: string, extra: Record<string, unknown> = {}) => ({
    id: "pocket-timer", name: "Pocket timer", version: "0.1.0", description: "A timer in the session header.", permissions: ["ui.add"], source: source("pocket-timer"),
    kind, previousVersion: null, added: ["ui.add"], removed: [], ...extra,
  });
  const catUpdate = { id: "composer-cat", name: "Composer cat", description: "A cat sits on the message box.", source: source("composer-cat"), version: "0.3.0", previousVersion: "0.2.0", permissions: ["ui.add", "sessions.read"], added: ["sessions.read"], removed: [] };
  const changed = () => window.__navigation.emit("plugins-status", structuredClone(listed));
  const fail = (kind: string, message: string) => Promise.reject({ kind, message });

  const fake = async (command: string, args: Record<string, unknown> = {}): Promise<unknown> => {
    if (!command.startsWith("plugins_") || ["plugins_asset", "plugins_wanted", "plugins_placements", "plugins_publish", "plugins_instances", "plugins_event", "plugins_visible"].includes(command)) return original(command, args);
    calls.push({ command, args });
    switch (command) {
      case "plugins_status": return structuredClone(listed);
      case "plugins_enable": {
        const row = find(args.id);
        if (row) { row.enabled = args.enabled; row.state = args.enabled ? "running" : "disabled"; }
        changed();
        return null;
      }
      case "plugins_stage_git": {
        if (args.repository === "owner/missing") return fail("git", "Git could not read that repository. It may not exist, or it may be private and need a sign-in.");
        if (args.repository === "owner/taken") return fail("already-installed", "A plugin with the id pocket-timer is already installed from another source. Remove it first.");
        return { stage: "11111111-1111-4111-8111-111111111111", preview: preview("new") };
      }
      case "plugins_stage_folder": return fail("no-manifest", "There is no beep-plugin.json in the repository. Check the folder, or leave it empty for the root.");
      case "plugins_check_update": return { state: "available", commit: "bbbbbbb8b3c2d1e0f9a8b7c6d5e4f30211223344" };
      case "plugins_stage_update": return { stage: "22222222-2222-4222-8222-222222222222", preview: preview("update", catUpdate) };
      case "plugins_review": return preview("allow", { id: "quote-board", name: "Quote board", version: "0.4.0", permissions: ["ui.add", "sessions.read"], added: ["sessions.read"] });
      case "plugins_allow": {
        const row = find(args.id);
        if (row) { row.state = "running"; row.approved = row.permissions; }
        changed();
        return null;
      }
      case "plugins_confirm": {
        const staged = typeof args.stage === "string" && args.stage.startsWith("2");
        if (staged) { const row = find("composer-cat"); if (row) { row.version = "0.3.0"; row.permissions = ["ui.add", "sessions.read"]; row.approved = ["ui.add", "sessions.read"]; } }
        else listed.push({ id: "pocket-timer", name: "Pocket timer", version: "0.1.0", description: "A timer in the session header.", permissions: ["ui.add"], approved: ["ui.add"], source: source("pocket-timer"), enabled: true, state: "running" });
        changed();
        return "pocket-timer";
      }
      case "plugins_remove": {
        if (options.failRemove) return fail("disk", "cannot delete the plugin's folder: permission denied");
        const index = listed.indexOf(find(args.id));
        if (index >= 0) listed.splice(index, 1);
        changed();
        return null;
      }
      default: return null;
    }
  };
  Reflect.set(core, "invoke", fake);
  Object.defineProperty(window, "__plugins", { value: { calls } });
}

async function openPlugins(page: Page, options: Partial<FakeOptions> = {}) {
  const errors = pageErrors(page);
  const folder = { path: "/srv/plugins/cat", parent: "/srv/plugins", exists: true, folders: [], more: 0, empty: false, origin: null, repos: [] };
  await openDesktop(page, false, { settings, list_machines: [], list_credentials: [], list_github_accounts: [], mac_control: { id: "local" }, list_machine_folder: folder });
  await page.evaluate(installFake, { plugins, failRemove: false, ...options });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await sheet.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Plugins", exact: true }).click();
  await expect(sheet.locator(".plg-page")).toBeVisible();
  await expect(sheet.locator(".dskel-rows")).toHaveCount(0);
  return { sheet, errors };
}

const calls = (page: Page, command: string) => page.evaluate(name => {
  const log: unknown = Reflect.get(window, "__plugins");
  return log !== null && typeof log === "object" && "calls" in log && Array.isArray(log.calls) ? log.calls.filter((call: { command: string }) => call.command === name).map((call: { args: unknown }) => call.args) : [];
}, command);

test("the list shows each plugin with its source, permissions and state", async ({ page }) => {
  const { sheet, errors } = await openPlugins(page);
  const row = (name: string) => sheet.getByRole("listitem").filter({ has: page.getByText(name, { exact: true }) });

  await expect(sheet.getByRole("listitem")).toHaveCount(6);
  const cat = row("Composer cat");
  await expect(cat).toContainText("0.2.0");
  await expect(cat).toContainText("A cat sits on the message box.");
  await expect(cat).toContainText("github.com/owner/composer-cat at 4f0915a");
  await expect(cat).toContainText("Add to the interface");
  await expect(cat).toContainText("On");
  await expect(row("Session notes")).toContainText("Local folder");
  await expect(row("Session notes")).toContainText("Read your sessions");
  await expect(row("Session notes")).toContainText("Waiting for its slot");
  await expect(row("Idle timer")).toContainText("github.com/owner/beep-plugins, plugins/idle-timer at 91c2e07");
  await expect(row("Idle timer")).toContainText("Off");
  await expect(row("Quote board")).toContainText("Needs approval");
  await expect(row("Tidy header")).toContainText("Built in");
  await expect(row("Dev sketch")).toContainText("Dev folder");
  await expect(row("Dev sketch")).toContainText("Stopped: used 612 ms of script time in a minute");

  await expect(sheet.getByRole("switch", { name: "Composer cat" })).toHaveAttribute("aria-checked", "true");
  await expect(sheet.getByRole("switch", { name: "Idle timer" })).toHaveAttribute("aria-checked", "false");
  await expect(sheet.getByRole("switch", { name: "Quote board" })).toBeDisabled();
  await expect(sheet.getByRole("button", { name: "Allow Quote board" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Allow Composer cat" })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Update Composer cat" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Update Session notes" })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Update Dev sketch" })).toHaveCount(0);
  await expect(sheet.getByRole("switch", { name: "Tidy header" })).toBeEnabled();
  await expect(sheet.getByRole("button", { name: "Remove Tidy header" })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Remove Dev sketch" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Update Tidy header" })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Install from Git…" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Install from folder…" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Open plugins folder" })).toBeVisible();
  expect(errors).toEqual([]);
});

test("a switch turns a plugin on and the list follows the desktop's change event", async ({ page }) => {
  const { sheet } = await openPlugins(page);
  const toggle = sheet.getByRole("switch", { name: "Idle timer" });

  await toggle.focus();
  await page.keyboard.press("Space");

  await expect(toggle).toHaveAttribute("aria-checked", "true");
  expect(await calls(page, "plugins_enable")).toEqual([{ id: "idle-timer", enabled: true }]);
  await expect(sheet.getByRole("listitem").filter({ hasText: "Idle timer" })).toContainText("On");

  // A pushed list is shown as it comes, without reading again; a null push (host not started) changes nothing.
  const reads = (await calls(page, "plugins_status")).length;
  await page.evaluate(() => window.__navigation.emit("plugins-status", null));
  await page.evaluate(() => window.__navigation.emit("plugins-status", []));
  await expect(sheet.getByRole("listitem").filter({ hasText: "Idle timer" })).toHaveCount(0);
  expect((await calls(page, "plugins_status")).length).toBe(reads);
});

test("Install from Git shows the permissions, then installs on Allow and install", async ({ page }) => {
  const { sheet, errors } = await openPlugins(page);

  await sheet.getByRole("button", { name: "Install from Git…" }).click();
  const form = sheet.getByRole("form", { name: "Install from Git" });
  await expect(form.getByLabel("Repository", { exact: true })).toBeFocused();
  await form.getByLabel("Repository", { exact: true }).fill("owner/pocket-timer");
  await form.getByLabel("Branch or tag (optional)").fill("main");
  await form.getByRole("button", { name: "Review plugin" }).click();

  const consent = page.getByRole("dialog", { name: "Install Pocket timer?" });
  await expect(consent).toBeVisible();
  await expect(consent).toContainText("Pocket timer 0.1.0");
  await expect(consent).toContainText("From github.com/owner/pocket-timer, commit 4f0915a");
  await expect(consent).toContainText("Add to the interface");
  await expect(consent).toContainText("Show text, buttons and images in the message box");
  await expect(consent.getByRole("button", { name: "Allow and install" })).toBeFocused();
  expect(await calls(page, "plugins_stage_git")).toEqual([{ repository: "owner/pocket-timer", reference: "main", folder: "" }]);
  expect(await calls(page, "plugins_confirm")).toEqual([]);

  await consent.getByRole("button", { name: "Allow and install" }).click();

  await expect(consent).toHaveCount(0);
  expect(await calls(page, "plugins_confirm")).toEqual([{ stage: "11111111-1111-4111-8111-111111111111" }]);
  await expect(sheet.getByRole("listitem")).toHaveCount(7);
  await expect(sheet.getByRole("listitem").filter({ hasText: "Pocket timer" })).toContainText("On");
  await expect(form).toHaveCount(0);
  expect(errors).toEqual([]);
});

test("cancelling or pressing Escape on the consent sheet throws the staged plugin away", async ({ page }) => {
  const { sheet } = await openPlugins(page);
  const stage = async () => {
    await sheet.getByRole("button", { name: "Install from Git…" }).click();
    await sheet.getByLabel("Repository", { exact: true }).fill("owner/pocket-timer");
    await sheet.getByRole("button", { name: "Review plugin" }).click();
    await expect(page.getByRole("dialog", { name: "Install Pocket timer?" })).toBeVisible();
  };

  await stage();
  await page.getByRole("dialog", { name: "Install Pocket timer?" }).getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("dialog", { name: "Install Pocket timer?" })).toHaveCount(0);
  await expect(sheet).toBeVisible();

  await stage();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "Install Pocket timer?" })).toHaveCount(0);
  await expect(sheet).toBeVisible();

  expect(await calls(page, "plugins_cancel")).toEqual([{ stage: "11111111-1111-4111-8111-111111111111" }, { stage: "11111111-1111-4111-8111-111111111111" }]);
  expect(await calls(page, "plugins_confirm")).toEqual([]);
  await expect(sheet.getByRole("listitem")).toHaveCount(6);
});

test("a typed error shows next to the form and keeps what was typed", async ({ page }) => {
  const { sheet } = await openPlugins(page);

  await sheet.getByRole("button", { name: "Install from Git…" }).click();
  await sheet.getByLabel("Repository", { exact: true }).fill("owner/missing");
  await sheet.getByRole("button", { name: "Review plugin" }).click();

  const form = sheet.getByRole("form", { name: "Install from Git" });
  await expect(form.getByRole("alert")).toHaveText("Git could not read that repository. It may not exist, or it may be private and need a sign-in.");
  await expect(form.getByLabel("Repository", { exact: true })).toHaveValue("owner/missing");
  await expect(page.getByRole("dialog", { name: /Install/ })).toHaveCount(0);

  await form.getByLabel("Repository", { exact: true }).fill("");
  await form.getByRole("button", { name: "Review plugin" }).click();
  await expect(form.getByRole("alert")).toHaveText("Enter a repository, such as owner/repo.");
  expect((await calls(page, "plugins_stage_git")).length).toBe(1);
});

test("the consent sheet shows an error from confirming and can be cancelled", async ({ page }) => {
  const { sheet } = await openPlugins(page);
  await page.evaluate(() => {
    const core = window.__TAURI__.core;
    const before: (command: string, args?: Record<string, unknown>) => Promise<unknown> = core.invoke.bind(core);
    Reflect.set(core, "invoke", (command: string, args?: Record<string, unknown>) => command === "plugins_confirm" ? Promise.reject({ kind: "already-installed", message: "A plugin with the id pocket-timer is already installed from another source. Remove it first." }) : before(command, args));
  });

  await sheet.getByRole("button", { name: "Install from Git…" }).click();
  await sheet.getByLabel("Repository", { exact: true }).fill("owner/pocket-timer");
  await sheet.getByRole("button", { name: "Review plugin" }).click();
  const consent = page.getByRole("dialog", { name: "Install Pocket timer?" });
  await consent.getByRole("button", { name: "Allow and install" }).click();

  await expect(consent.getByRole("alert")).toContainText("already installed from another source");
  await expect(consent.getByRole("button", { name: "Allow and install" })).toBeEnabled();
  await consent.getByRole("button", { name: "Cancel" }).click();
  await expect(consent).toHaveCount(0);
});

test("a folder without a manifest fails next to the install buttons", async ({ page }) => {
  const { sheet } = await openPlugins(page);

  await sheet.getByRole("button", { name: "Install from folder…" }).click();
  await page.getByRole("option", { name: /Use this folder/ }).click();

  await expect(sheet.getByRole("alert")).toContainText("There is no beep-plugin.json");
  expect(await calls(page, "plugins_stage_folder")).toEqual([{ folder: "/srv/plugins/cat" }]);
  await expect(sheet.getByRole("button", { name: "Install from folder…" })).toBeEnabled();
});

test("Update shows what the new version asks for before anything changes", async ({ page }) => {
  const { sheet } = await openPlugins(page);

  await sheet.getByRole("button", { name: "Update Composer cat" }).click();

  const consent = page.getByRole("dialog", { name: "Update Composer cat?" });
  await expect(consent).toContainText("Composer cat 0.2.0 to 0.3.0");
  await expect(consent.getByRole("listitem").filter({ hasText: "Read your sessions" })).toContainText("New");
  await expect(consent.getByRole("listitem").filter({ hasText: "Add to the interface" })).not.toContainText("New");
  expect(await calls(page, "plugins_confirm")).toEqual([]);

  await consent.getByRole("button", { name: "Allow and update" }).click();

  await expect(consent).toHaveCount(0);
  expect(await calls(page, "plugins_confirm")).toEqual([{ stage: "22222222-2222-4222-8222-222222222222" }]);
  await expect(sheet.getByRole("listitem").filter({ hasText: "Composer cat" })).toContainText("0.3.0");
});

test("Allow asks first, then allows the plugin that needed approval", async ({ page }) => {
  const { sheet } = await openPlugins(page);

  await sheet.getByRole("button", { name: "Allow Quote board" }).click();
  const consent = page.getByRole("dialog", { name: "Allow Quote board?" });
  await expect(consent).toContainText("Read your sessions");
  expect(await calls(page, "plugins_allow")).toEqual([]);
  await consent.getByRole("button", { name: "Allow", exact: true }).click();

  await expect(consent).toHaveCount(0);
  expect(await calls(page, "plugins_allow")).toEqual([{ id: "quote-board" }]);
  await expect(sheet.getByRole("listitem").filter({ hasText: "Quote board" })).toContainText("On");
  await expect(sheet.getByRole("switch", { name: "Quote board" })).toBeEnabled();
});

test("Remove names the plugin, can be cancelled and then removes it", async ({ page }) => {
  const { sheet } = await openPlugins(page);

  await sheet.getByRole("button", { name: "Remove Idle timer" }).click();
  const confirm = page.getByRole("dialog", { name: "Remove Idle timer?" });
  await expect(confirm).toBeVisible();
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(sheet.getByRole("listitem")).toHaveCount(6);
  expect(await calls(page, "plugins_remove")).toEqual([]);

  await sheet.getByRole("button", { name: "Remove Idle timer" }).click();
  await page.getByRole("dialog", { name: "Remove Idle timer?" }).getByRole("button", { name: "Remove", exact: true }).click();

  await expect(sheet.getByRole("listitem")).toHaveCount(5);
  await expect(sheet.getByRole("listitem").filter({ hasText: "Idle timer" })).toHaveCount(0);
  expect(await calls(page, "plugins_remove")).toEqual([{ id: "idle-timer" }]);
});

test("a failed removal shows its message in the sheet and keeps the plugin", async ({ page }) => {
  const { sheet } = await openPlugins(page, { failRemove: true });

  await sheet.getByRole("button", { name: "Remove Idle timer" }).click();
  const confirm = page.getByRole("dialog", { name: "Remove Idle timer?" });
  await confirm.getByRole("button", { name: "Remove", exact: true }).click();

  await expect(confirm).toContainText("cannot delete the plugin's folder: permission denied");
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(sheet.getByRole("listitem")).toHaveCount(6);
});

test("an empty list says what plugins are and offers both installs", async ({ page }) => {
  const { sheet } = await openPlugins(page, { plugins: [] });

  await expect(sheet.getByText("No plugins yet")).toBeVisible();
  await expect(sheet.getByText("Plugins are small scripts that add to Beep's interface")).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Install from Git…" })).toBeVisible();
  await expect(sheet.getByRole("button", { name: "Install from folder…" })).toBeVisible();
  await sheet.getByRole("button", { name: "Open plugins folder" }).click();
  expect(await calls(page, "plugins_open_folder")).toHaveLength(1);
});

test("a list that cannot be read says so and tries again", async ({ page }) => {
  const errors = pageErrors(page);
  const folder = { path: "/srv/plugins/cat", parent: "/srv/plugins", exists: true, folders: [], more: 0, empty: false, origin: null, repos: [] };
  await openDesktop(page, false, { settings, list_machines: [], list_credentials: [], list_github_accounts: [], mac_control: { id: "local" }, list_machine_folder: folder });
  await page.evaluate(() => {
    const core = window.__TAURI__.core;
    const before: (command: string, args?: Record<string, unknown>) => Promise<unknown> = core.invoke.bind(core);
    let failing = true;
    Reflect.set(core, "invoke", (command: string, args?: Record<string, unknown>) => {
      if (command !== "plugins_status") return before(command, args);
      if (failing) { failing = false; return Promise.reject({ kind: "not-running", message: "Plugins are not running, so they cannot be changed." }); }
      return Promise.resolve([]);
    });
  });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await sheet.getByRole("navigation", { name: "Settings pages" }).getByRole("button", { name: "Plugins", exact: true }).click();

  await expect(sheet.getByRole("alert")).toHaveText("Plugins are not running, so they cannot be changed.");
  await sheet.getByRole("button", { name: "Try again" }).click();
  await expect(sheet.getByText("No plugins yet")).toBeVisible();
  expect(errors.filter(text => !text.includes("not-running") && !text.includes("Plugins are not running"))).toEqual([]);
});

test("a phone lists no Plugins page", async ({ page }) => {
  const folder = { path: "/srv/plugins/cat", parent: "/srv/plugins", exists: true, folders: [], more: 0, empty: false, origin: null, repos: [] };
  await openDesktop(page, false, { settings, list_machines: [], list_credentials: [], list_github_accounts: [], mac_control: { id: "local" }, list_machine_folder: folder });
  await page.evaluate(() => { Reflect.set(window, "__BEEP_PHONE__", true); });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await expect(sheet.getByRole("button", { name: "Credentials", exact: true })).toBeVisible();

  await expect(sheet.getByRole("button", { name: "This computer", exact: true })).toHaveCount(0);
  await expect(sheet.getByRole("button", { name: "Plugins", exact: true })).toHaveCount(0);
});

for (const scheme of ["dark"] as const) {
  test(`screenshots of the page and the consent sheet in the ${scheme} theme`, async ({ page }, info) => {
    test.skip(process.env.PLUGIN_SLOT_SHOTS !== "1" || info.project.name !== "macOS", "set PLUGIN_SLOT_SHOTS=1 for the pictures");
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
    const { sheet } = await openPlugins(page);

    await page.screenshot({ path: docs("plugins-settings.png") });

    await sheet.getByRole("button", { name: "Install from Git…" }).click();
    await sheet.getByLabel("Repository", { exact: true }).fill("owner/missing");
    await sheet.getByRole("button", { name: "Review plugin" }).click();
    await expect(sheet.getByRole("alert")).toBeVisible();
    await page.screenshot({ path: docs("plugins-git-form.png") });
    await sheet.getByRole("button", { name: "Cancel" }).click();

    await sheet.getByRole("button", { name: "Update Composer cat" }).click();
    await expect(page.getByRole("dialog", { name: "Update Composer cat?" })).toBeVisible();
    await page.screenshot({ path: docs("plugins-consent.png") });
  });
}

test("a skin plugin is picked in Appearance and restyles the window", async ({ page }) => {
  const errors = pageErrors(page);
  const flat = status("flat", "Flat", { permissions: ["style"], approved: ["style"], state: "waiting", style: { "--radius": "0", "--surface": "#202020" } });
  await openDesktop(page, false, { settings, list_machines: [], list_credentials: [], list_github_accounts: [], plugins_status: [flat] });
  await page.locator("#side").getByRole("button", { name: "Settings", exact: true }).click();
  const sheet = page.getByRole("dialog", { name: "Settings" });
  await sheet.getByRole("button", { name: "Appearance", exact: true }).click();

  const pick = sheet.locator('[data-skin-pick="plugin:flat"]');
  await expect(pick).toHaveText("Flat");
  await pick.click();

  await expect(pick).toHaveAttribute("aria-pressed", "true");
  const radius = () => page.evaluate(() => document.documentElement.style.getPropertyValue("--radius"));
  await expect.poll(radius).toBe("0");

  await sheet.locator('[data-skin-pick="paper"]').click();
  await expect.poll(radius).toBe("");
  await expect.poll(() => page.evaluate(() => document.documentElement.dataset.skin)).toBe("paper");
  expect(errors).toEqual([]);
});
