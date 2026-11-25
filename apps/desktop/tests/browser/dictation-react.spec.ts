import { expect, test, type Page } from "@playwright/test";
import { openDesktop, openStartForm, sessionSettled } from "./harness.ts";

const hosts = [{ machine_id: "build-box", machine_name: "build-box", agents: ["codex", "claude"], workspace: "/srv/project", home: "/path/to/work", capabilities: [] }];

/// A microphone that plays a tone, and a dictation the test speaks into with `window.say(event)`.
async function fakeDictation(page: Page) {
  await page.evaluate(() => {
    navigator.mediaDevices.getUserMedia = async () => {
      const context = new AudioContext(), tone = context.createOscillator(), out = context.createMediaStreamDestination();
      tone.connect(out);
      tone.start();
      return out.stream;
    };
    const invoke = window.__TAURI__.core.invoke;
    window.__TAURI__.core.invoke = async <T>(command: string, args?: Record<string, unknown>) => {
      if (command !== "dictation_start") return invoke<T>(command, args);
      const channel = args?.onEvent;
      if (typeof channel !== "object" || channel === null || !("onmessage" in channel)) throw new Error("no channel");
      Reflect.set(window, "say", (event: unknown) => { if (typeof channel.onmessage === "function") channel.onmessage(event); });
      await invoke(command, args);
      return "6f1d3b7e-6c55-4c1a-9a0e-3d2f1b0c9a11" as T;
    };
  });
}

test("the start composer writes what is said at the caret and stops on the mic", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, dictation_settings_get: { enabled: true, model: null } });
  await fakeDictation(page);
  await openStartForm(page);
  const box = page.locator("#agbox");
  const input = page.locator("#agprompt");
  await input.fill("Please ");
  const resting = (await box.boundingBox())!.height;
  const mic = page.getByRole("button", { name: "Dictate prompt" });
  await mic.click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_start" && call.args.model === null))).toBe(true);
  await page.evaluate(() => {
    const say = Reflect.get(window, "say");
    if (typeof say !== "function") throw new Error("not started");
    say({ kind: "ready" });
    say({ kind: "transcript", committed: "", partial: "fix the" });
    say({ kind: "transcript", committed: "fix the login", partial: "crash" });
  });
  await expect(input).toHaveValue("Please fix the login crash");
  await expect(page.locator(".dict-status.listening")).toBeVisible();
  await expect.poll(async () => (await box.boundingBox())?.height ?? 0).toBeGreaterThan(resting + 25);
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_audio" && typeof call.args.pcmBase64 === "string"))).toBe(true);
  await page.getByRole("button", { name: "Stop dictation (Esc cancels)" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_stop"))).toBe(true);
  await page.evaluate(() => { const say = Reflect.get(window, "say"); if (typeof say === "function") say({ kind: "done", nothing_heard: false }); });
  await expect(page.getByRole("button", { name: "Dictate prompt" })).toBeEnabled();
  await expect(input).toHaveValue("Please fix the login crash");
  await expect.poll(async () => Math.abs(((await box.boundingBox())?.height ?? 999) - resting)).toBeLessThan(2);
});

test("a provider error shows beside the field and nothing is written", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, dictation_settings_get: { enabled: true, model: null } });
  await fakeDictation(page);
  await openStartForm(page);
  await page.getByRole("button", { name: "Dictate prompt" }).click();
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "say") === "function")).toBe(true);
  await page.evaluate(() => { const say = Reflect.get(window, "say"); if (typeof say === "function") say({ kind: "error", message: "Add your ElevenLabs API key in Settings first." }); });
  await expect(page.locator(".dict-status.error")).toHaveText("Add your ElevenLabs API key in Settings first.");
  await expect(page.locator("#agprompt")).toHaveValue("");
});

test("no mic button while dictation is off", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, dictation_settings_get: { enabled: false, model: null } });
  await openStartForm(page);
  await expect(page.getByRole("button", { name: "Dictate prompt" })).toHaveCount(0);
});

test("the session composer expands to clear its controls during dictation, keeps deleted words deleted, and shrinks back", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, dictation_settings_get: { enabled: true, model: null } });
  await fakeDictation(page);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await sessionSettled(page);

  const box = page.locator("#agfield");
  const input = page.locator("#agmsg");
  await expect(input).toBeVisible();
  const resting = (await box.boundingBox())!.height;
  expect(resting).toBeLessThan(50);

  await page.getByRole("button", { name: "Dictate prompt" }).click();
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "say") === "function")).toBe(true);
  await page.evaluate(() => {
    const say = Reflect.get(window, "say");
    if (typeof say === "function") {
      say({ kind: "ready" });
      say({ kind: "transcript", committed: "I am dictating.", partial: "" });
    }
  });

  await expect(input).toHaveValue("I am dictating.");
  await expect.poll(async () => (await box.boundingBox())?.height ?? 0).toBeGreaterThan(resting + 55);

  const statusBox = (await page.locator("#agfield .dict-status.listening").boundingBox())!;
  const stopDictBox = (await page.getByRole("button", { name: "Stop dictation (Esc cancels)" }).boundingBox())!;
  const inputBox = (await input.boundingBox())!;
  expect(statusBox.y + statusBox.height).toBeLessThan(stopDictBox.y);
  expect(inputBox.y + inputBox.height).toBeLessThanOrEqual(stopDictBox.y);

  // Delete "dictating." while dictation is still running, then speak another phrase.
  await input.focus();
  for (let i = 0; i < "dictating.".length; i++) await input.press("Backspace");
  await expect(input).toHaveValue("I am ");

  await page.evaluate(() => {
    const say = Reflect.get(window, "say");
    if (typeof say === "function") say({ kind: "transcript", committed: "I am dictating. ok.", partial: "" });
  });
  await expect(input).toHaveValue("I am ok.");

  await page.getByRole("button", { name: "Stop dictation (Esc cancels)" }).click();
  await page.evaluate(() => { const say = Reflect.get(window, "say"); if (typeof say === "function") say({ kind: "done", nothing_heard: false }); });
  await expect.poll(async () => Math.abs(((await box.boundingBox())?.height ?? 999) - resting)).toBeLessThan(2);
});
