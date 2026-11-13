import { expect, test } from "@playwright/test";
import { openDesktop } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box" };
const pixel = { id: "pixel", name: "Pixel 9", platform: "Android", state: "On", os: "Android 16" };

test("the Device tab shows a device live, takes touches, keys and buttons, and stops when hidden", async ({ page }) => {
  await openDesktop(page, true, { device_hosts: [host], device_platforms: ["Android"], device_list: [{ ...host, devices: [pixel], error: null }] });
  await page.evaluate(() => {
    const internals = window.__TAURI_INTERNALS__;
    internals.transformCallback = callback => { Reflect.set(window, "deviceChannel", callback); return 99; };
    const invoke = window.__TAURI__.core.invoke;
    // The attach is recorded and stays open, as a live stream does.
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      const answer = invoke<T>(command, args);
      return command === "device_attach" ? new Promise<T>(() => {}) : answer;
    };
    window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } });
  });

  await page.getByRole("tab", { name: "Device" }).click();
  await expect(page.getByRole("combobox", { name: "Device" })).toHaveValue("build-box\npixel");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().find(call => call.command === "device_attach")?.args.id)).toBe("pixel");

  // A 100 by 200 frame, as the node sends it: its size, then the JPEG.
  await page.evaluate(async () => {
    const canvas = document.createElement("canvas");
    canvas.width = 100;
    canvas.height = 200;
    const g = canvas.getContext("2d");
    if (g) { g.fillStyle = "#3366aa"; g.fillRect(0, 0, 100, 200); }
    const blob = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, "image/jpeg"));
    const jpeg = new Uint8Array(await (blob ?? new Blob()).arrayBuffer());
    const bytes = new Uint8Array(8 + jpeg.length);
    const view = new DataView(bytes.buffer);
    view.setUint32(0, 100, true);
    view.setUint32(4, 200, true);
    bytes.set(jpeg, 8);
    const deliver = Reflect.get(window, "deviceChannel") as (call: { message: unknown; index: number }) => void;
    deliver({ message: { kind: "attached" }, index: 0 });
    deliver({ message: bytes.buffer, index: 1 });
  });

  const screen = page.locator("canvas.agdevscreen");
  await expect(screen).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "device_drawn"))).toBe(true);

  const box = await screen.boundingBox();
  if (!box) throw new Error("the screen has no box");
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await expect(page.getByRole("textbox", { name: "Type into the device" })).toBeFocused();
  await expect(page.locator(".agdevstage")).toHaveCSS("box-shadow", "none");
  const inputs = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_input").map(call => call.args.input));
  // Browser pointer coordinates can round to the nearest CSS pixel.
  await expect.poll(inputs).toEqual([{ Touch: { phase: "Down", x: expect.closeTo(0.5, 2), y: expect.closeTo(0.5, 2) } }, { Touch: { phase: "Up", x: expect.closeTo(0.5, 2), y: expect.closeTo(0.5, 2) } }]);

  await page.keyboard.type("hi");
  await page.keyboard.press("Enter");
  await page.locator(".agdevkeys").getByRole("button", { name: "Home" }).click();
  await expect.poll(async () => (await inputs()).slice(2)).toEqual([{ Text: { text: "h" } }, { Text: { text: "i" } }, { Key: { key: "Return" } }, { Button: { button: "Home" } }]);

  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "device_detach"))).toBe(true);
});

test("no Device tab while no machine serves devices", async ({ page }) => {
  await openDesktop(page, true);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await expect(page.getByRole("tab", { name: "Files", exact: true })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Device" })).toHaveCount(0);
});

const session = { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } as const;

test("no Device tab for a checkout that is no mobile app", async ({ page }) => {
  await openDesktop(page, true, { device_hosts: [host], device_platforms: [], device_list: [{ ...host, devices: [pixel], error: null }] });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await expect(page.getByRole("tab", { name: "Files", exact: true })).toBeVisible();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "device_platforms"))).toBe(true);
  await expect(page.getByRole("tab", { name: "Device" })).toHaveCount(0);
});

test("an iOS project with no simulator says so in one line", async ({ page }) => {
  await openDesktop(page, true, { device_hosts: [host], device_platforms: ["Ios"], device_list: [{ ...host, devices: [pixel], error: null }] });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await page.getByRole("tab", { name: "Device" }).click();
  await expect(page.locator(".agdevmiss")).toHaveText("No machine has an iOS simulator. A Mac with Xcode and idb_companion can run one.");
  await expect(page.getByRole("combobox", { name: "Device" })).toHaveValue("");
});
