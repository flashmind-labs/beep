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

test("holding Option on screen runs push-to-talk and stops on release, while taps and combos do not", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, dictation_settings_get: { enabled: true, model: null } });
  await fakeDictation(page);
  await page.evaluate(() => window.__navigation.open({ agent: { kind: "agent", machine_id: "build-box", machine_name: "build-box", id: "notice-one", agent: "codex", title: "Example", cwd: "/srv/project", status: "working", doing: null, started_at: 1, last_activity: 1, external: false } }));
  await sessionSettled(page);

  const input = page.locator("#agmsg");
  await expect(input).toBeVisible();
  // Blur the composer to verify holding Option works anywhere on the session screen.
  await input.evaluate((el: HTMLTextAreaElement) => el.blur());

  // Quick tap of Option (< 220 ms) never starts dictation.
  await page.keyboard.down("Alt");
  await page.waitForTimeout(60);
  await page.keyboard.up("Alt");
  await page.waitForTimeout(260);
  expect(await page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_start"))).toBe(false);

  // Holding Option while another input has focus leaves it alone.
  const filter = page.getByRole("searchbox", { name: "Filter sessions" });
  await filter.focus();
  await page.keyboard.down("Alt");
  await page.waitForTimeout(300);
  await page.keyboard.up("Alt");
  expect(await page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_start"))).toBe(false);
  await filter.evaluate((el: HTMLInputElement) => el.blur());

  // Holding Option alone on the session screen focuses the composer, listens, and stops on release.
  await page.keyboard.down("Alt");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_start"))).toBe(true);
  await expect(input).toBeFocused();
  await page.evaluate(() => {
    const say = Reflect.get(window, "say");
    if (typeof say === "function") {
      say({ kind: "ready" });
      say({ kind: "transcript", committed: "push to talk works", partial: "" });
    }
  });
  await expect(input).toHaveValue("push to talk works");
  expect(await page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_stop"))).toBe(false);

  await page.keyboard.up("Alt");
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_stop"))).toBe(true);
  await page.evaluate(() => { const say = Reflect.get(window, "say"); if (typeof say === "function") say({ kind: "done", nothing_heard: false }); });
  await expect(page.getByRole("button", { name: "Dictate prompt" })).toBeEnabled();
  await expect(input).toHaveValue("push to talk works");
});

test("pauses playing audio before opening the mic and resumes it only after leaving call mode", async ({ page }) => {
  await openDesktop(page, false, { agent_hosts: hosts, dictation_settings_get: { enabled: true, model: null } });
  await fakeDictation(page);
  await openStartForm(page);

  await page.evaluate(() => {
    const events: string[] = [];
    Reflect.set(window, "audioEvents", events);

    const makeTrack = (id: string, initiallyPaused: boolean) => {
      const audio = document.createElement("audio");
      audio.id = id;
      let isPaused = initiallyPaused;
      Object.defineProperty(audio, "paused", { get: () => isPaused });
      Object.defineProperty(audio, "ended", { get: () => false });
      audio.pause = () => {
        isPaused = true;
        events.push(`${id}:pause`);
      };
      audio.play = async () => {
        isPaused = false;
        events.push(`${id}:play`);
      };
      document.body.append(audio);
    };

    makeTrack("music-playing", false);
    makeTrack("music-already-paused", true);

    const getUserMedia = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
    navigator.mediaDevices.getUserMedia = async constraints => {
      events.push("mic:open");
      return getUserMedia(constraints);
    };

    window.__navigation.hold("dictation_mic_leave");
  });

  await page.getByRole("button", { name: "Dictate prompt" }).click();
  await expect.poll(() => page.evaluate(() => typeof Reflect.get(window, "say") === "function")).toBe(true);

  const startOrder = await page.evaluate(() => ({
    events: Reflect.get(window, "audioEvents") as string[],
    enterArgs: window.__navigation.calls().find(call => call.command === "dictation_mic_enter")?.args ?? null,
    callNames: window.__navigation.calls().map(call => call.command),
  }));
  expect(startOrder.events).toEqual(["music-playing:pause", "mic:open"]);
  expect(startOrder.enterArgs).toEqual({ pagePaused: true });
  expect(startOrder.callNames.indexOf("dictation_mic_enter")).toBeLessThan(startOrder.callNames.indexOf("dictation_start"));

  await page.evaluate(() => {
    const say = Reflect.get(window, "say");
    if (typeof say === "function") {
      say({ kind: "ready" });
      say({ kind: "transcript", committed: "hello", partial: "" });
    }
  });

  await page.getByRole("button", { name: "Stop dictation (Esc cancels)" }).click();
  await expect.poll(() => page.evaluate(() => window.__navigation.calls().some(call => call.command === "dictation_mic_leave"))).toBe(true);

  // While headphones are still in call mode (`dictation_mic_leave` held), music remains paused.
  expect(await page.evaluate(() => Reflect.get(window, "audioEvents"))).toEqual(["music-playing:pause", "mic:open"]);

  // Once the output device returns to non-call mode, only the track dictation paused resumes.
  await page.evaluate(() => window.__navigation.release("dictation_mic_leave"));
  await expect.poll(() => page.evaluate(() => Reflect.get(window, "audioEvents"))).toEqual([
    "music-playing:pause",
    "mic:open",
    "music-playing:play",
  ]);
});
