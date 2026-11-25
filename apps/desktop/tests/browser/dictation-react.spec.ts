import { expect, test, type Page } from "@playwright/test";
import { openDesktop, openStartForm } from "./harness.ts";

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
  const input = page.locator("#agprompt");
  await input.fill("Please ");
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
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_audio" && typeof call.args.pcmBase64 === "string"))).toBe(true);
  await page.getByRole("button", { name: "Stop dictation (Esc cancels)" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_stop"))).toBe(true);
  await page.evaluate(() => { const say = Reflect.get(window, "say"); if (typeof say === "function") say({ kind: "done", nothing_heard: false }); });
  await expect(page.getByRole("button", { name: "Dictate prompt" })).toBeEnabled();
  await expect(input).toHaveValue("Please fix the login crash");
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
