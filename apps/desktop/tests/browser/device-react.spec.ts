import { expect, test, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { openDesktop } from "./harness.ts";

const host = { machine_id: "build-box", machine_name: "build-box" };
const pixel = { id: "pixel", name: "Pixel 9", platform: "Android", state: "On", os: "Android 16", phone: false };

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
  await expect(page.getByRole("button", { name: "Device Pixel 9" })).toBeVisible();
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

test("no Device tab while no machine serves devices for a mobile project", async ({ page }) => {
  await openDesktop(page, true, { device_hosts: [], device_platforms: ["Android"] });
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
  await expect(page.getByRole("button", { name: "Device", exact: true })).toBeVisible();
});

test("a machine still listing says so, not that it has no devices", async ({ page }) => {
  await openDesktop(page, true, { device_hosts: [host], device_platforms: ["Ios"], device_list: [{ ...host, devices: [], error: null, loading: true }] });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await page.getByRole("tab", { name: "Device" }).click();
  await expect(page.locator(".cc-empty")).toHaveText("Listing devices on build-box…");
  await expect(page.getByText("No emulator or simulator")).toHaveCount(0);
  await expect(page.getByText("No machine has an iOS simulator")).toHaveCount(0);
});

test("a phone shows without Boot or Shut down", async ({ page }) => {
  const phone = { ...pixel, id: "R5CT1234ABC", name: "Pixel 8 Pro", phone: true };
  await openDesktop(page, true, { device_hosts: [host], device_platforms: ["Android"], device_list: [{ ...host, devices: [phone], error: null }] });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await page.getByRole("tab", { name: "Device" }).click();
  await expect(page.getByRole("button", { name: "Device Pixel 8 Pro" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Boot|Shut down/ })).toHaveCount(0);
});

async function openVideoDevice(page: Page, unavailable = false) {
  await openDesktop(page, true, { device_hosts: [host], device_platforms: ["Android"], device_list: [{ ...host, devices: [pixel], error: null }] });
  await page.evaluate(({ agent, unavailable }) => {
    window.__TAURI_INTERNALS__.transformCallback = callback => { Reflect.set(window, "deviceChannel", callback); return 99; };
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      const answer = invoke<T>(command, args);
      if (command === "device_attach") {
        return unavailable && args?.video === true ? Promise.reject(new Error("Video capture is unavailable.")) : new Promise<T>(() => {});
      }
      return answer;
    };
    window.__navigation.open({ agent });
  }, { agent: session, unavailable });
  await page.getByRole("tab", { name: "Device" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_attach").length)).toBe(unavailable ? 2 : 1);
}

async function deliverVideo(page: Page, landscape = false) {
  const consumed = await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_drawn").length);
  await page.evaluate(() => {
    const deliver = Reflect.get(window, "deviceChannel");
    if (typeof deliver !== "function") throw new Error("missing device channel");
    deliver({ message: { kind: "attached" }, index: 0 });
    deliver({ message: { kind: "video", fps: 60 }, index: 1 });
  });
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_drawn").length)).toBe(consumed + 1);
  const fixture = landscape ? "../fixtures/device-video-landscape.h264" : "../fixtures/device-video.h264";
  const encoded = Array.from(readFileSync(new URL(fixture, import.meta.url)));
  await page.evaluate(data => {
    const deliver = Reflect.get(window, "deviceChannel");
    if (typeof deliver !== "function") throw new Error("missing device channel");
    let index = 2;
    // Deliberately split NAL units and start codes across transport chunks.
    for (let start = 0; start < data.length; start += 997) {
      const piece = data.slice(start, start + 997);
      const bytes = new Uint8Array(8 + piece.length);
      new DataView(bytes.buffer).setUint32(0, 0x31444956, true);
      bytes.set(piece, 8);
      deliver({ message: bytes.buffer, index: index++ });
    }
  }, encoded);
}

test("native H.264 bytes play without JPEG conversion and retain touch coordinates", async ({ page }) => {
  await openVideoDevice(page);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().find(call => call.command === "device_attach")?.args.video)).toBe(true);
  await deliverVideo(page);
  const video = page.locator("video.agdevvideo");
  await expect(video).toBeVisible();
  await expect.poll(() => video.evaluate(node => node instanceof HTMLVideoElement ? node.videoWidth : 0)).toBe(100);
  await expect.poll(() => video.evaluate(node => node instanceof HTMLVideoElement ? node.currentTime : 0)).toBeGreaterThan(0);
  expect(await page.locator(".agpview.device").innerText()).toMatchSnapshot("device-video.txt");
  const box = await video.boundingBox();
  if (!box) throw new Error("video has no box");
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_input").map(call => call.args.input))).toEqual([
    { Touch: { phase: "Down", x: expect.closeTo(0.5, 2), y: expect.closeTo(0.5, 2) } },
    { Touch: { phase: "Up", x: expect.closeTo(0.5, 2), y: expect.closeTo(0.5, 2) } },
  ]);
  await page.getByRole("tab", { name: "Files", exact: true }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "device_detach"))).toBe(true);
});

test("a client without a video decoder requests JPEG directly", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "MediaSource", { value: undefined });
    Object.defineProperty(window, "ManagedMediaSource", { value: undefined });
  });
  await openVideoDevice(page);
  expect(await page.evaluate(() => window.__navigation.calls().find(call => call.command === "device_attach")?.args.video)).toBe(false);
});

test("a video decoder failure reattaches once with JPEG", async ({ page }) => {
  await openVideoDevice(page);
  await deliverVideo(page);
  await expect(page.locator("video.agdevvideo")).toBeVisible();
  await page.locator("video.agdevvideo").dispatchEvent("error");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_attach").map(call => call.args.video))).toEqual([true, false]);
});

test("unsupported native capture retries with JPEG", async ({ page }) => {
  await openVideoDevice(page, true);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_attach").map(call => call.args.video))).toEqual([true, false]);
});

test("an encoder restart takes new dimensions and keyframes without losing the view", async ({ page }) => {
  await openVideoDevice(page);
  const video = page.locator("video.agdevvideo");
  await deliverVideo(page);
  await expect.poll(() => video.evaluate(node => node instanceof HTMLVideoElement ? node.videoWidth : 0)).toBe(100);
  await expect.poll(() => video.evaluate(node => node instanceof HTMLVideoElement ? node.currentTime : 0)).toBeGreaterThan(0);
  await deliverVideo(page, true);
  await expect.poll(() => video.evaluate(node => node instanceof HTMLVideoElement ? node.videoWidth : 0)).toBe(200);
  await expect.poll(() => video.evaluate(node => node instanceof HTMLVideoElement ? node.currentTime : 0)).toBeGreaterThan(0);
  expect(await page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_attach").length)).toBe(1);
});

test("a node that pushes its devices is not read again, and one that refuses the topic still is", async ({ page }) => {
  const second = { ...pixel, id: "tablet", name: "Tablet" };
  await openDesktop(page, true, { device_hosts: [host], device_platforms: ["Android"], device_list: [{ ...host, devices: [pixel], error: null }] });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await page.getByRole("tab", { name: "Device" }).click();
  await expect(page.getByRole("button", { name: "Device Pixel 9" })).toBeVisible();
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_list").length);
  const before = await reads();
  await page.evaluate(device => window.__navigation.publish("device-list.*", null, { Devices: [device] }, "build-box"), second);
  await expect(page.getByRole("button", { name: "Device Tablet" })).toBeVisible();
  expect(await reads()).toBe(before);

  await page.evaluate(() => window.__navigation.publish("device-list.*", null, { error: "no state has been published for this topic" }, "build-box"));
  await expect.poll(reads).toBeGreaterThan(before);
  await expect(page.getByRole("button", { name: "Device Pixel 9" })).toBeVisible();
});

test("a pushed boot reaches the live screen without a read, and a node with only the old topic is read on its ping", async ({ page }) => {
  const off = { ...pixel, state: "Off" };
  await openDesktop(page, true, { device_hosts: [host], device_platforms: ["Android"], device_list: [{ ...host, devices: [off], error: null }] });
  await page.evaluate(() => {
    window.__TAURI_INTERNALS__.transformCallback = () => 99;
    const invoke = window.__TAURI__.core.invoke;
    // The attach stays open, as a live stream does.
    window.__TAURI__.core.invoke = <T>(command: string, args?: Record<string, unknown>) => {
      const answer = invoke<T>(command, args);
      return command === "device_attach" ? new Promise<T>(() => {}) : answer;
    };
  });
  await page.evaluate(agent => window.__navigation.open({ agent }), session);
  await page.getByRole("tab", { name: "Device" }).click();
  await expect(page.getByRole("button", { name: "Device Pixel 9" })).toBeVisible();
  const reads = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_list").length);
  const attaches = () => page.evaluate(() => window.__navigation.calls().filter(call => call.command === "device_attach").length);

  await page.evaluate(device => window.__navigation.publish("device-list.*", null, { Devices: [device] }, "build-box"), { ...pixel, state: "Booting" });
  await expect(page.getByText("Booting Pixel 9")).toBeVisible();
  await page.evaluate(device => window.__navigation.publish("device-list.*", null, { Devices: [device] }, "build-box"), pixel);
  await expect.poll(attaches).toBe(1);
  const before = await reads();

  await page.evaluate(() => window.__navigation.publish("device-list.*", null, { error: "no state has been published for this topic" }, "build-box"));
  await expect.poll(reads).toBeGreaterThan(before);
  const refused = await reads();
  await page.evaluate(() => window.__navigation.publish("devices.*", null, { error: "no state has been published for this topic" }, "build-box"));
  await expect.poll(reads).toBeGreaterThan(refused);
});
