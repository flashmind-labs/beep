import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeStartComposer } from "../src/agents/start-composer.ts";
import { startAttachments } from "../src/agents/start-attachments.ts";

test("both start layouts accept video drops, expose a file picker, and protect pending files while starting", async () => {
  for (const presentation of ["compact", "centered"] as const) {
    const events = new Map<HTMLElement, Map<string, (event: unknown) => void>>();
    const proto = Object.getPrototypeOf(document.createElement("div"));
    const listen = proto.addEventListener;
    proto.addEventListener = function(this: HTMLElement, name: string, callback: (event: unknown) => void) {
      if (!events.has(this)) events.set(this, new Map());
      events.get(this)!.set(name, callback);
    };
    let c: ReturnType<typeof makeStartComposer>, attachments: ReturnType<typeof startAttachments>;
    const errors: string[] = [];

    try {
      c = makeStartComposer({ presentation });
      attachments = startAttachments(c, e => errors.push(e), () => {});
    } finally { proto.addEventListener = listen; }

    const send = (el: HTMLElement, type: string, event: unknown = {}) => events.get(el)?.get(type)?.(event);
    const input = c.form.querySelector<HTMLInputElement>('input');
    assert.ok(input);
    assert.equal(input.type, "file");
    assert.equal(input.multiple, true);
    assert.equal(input.hidden, true);
    const button = [...c.form.querySelectorAll<HTMLButtonElement>("button")].find(b => b.getAttribute("aria-label") === "Attach files");
    assert.ok(button);
    assert.equal(button.type, "button", "opening the picker must not start the agent");
    let picked = 0;
    input.click = () => { picked++; };
    send(button, "click");
    assert.equal(picked, 1);
    let prevented = 0;
    send(c.form, "drop", {
      dataTransfer: { files: [new File(["video"], "screen recording.mov", { type: "video/quicktime" })] },
      preventDefault() { prevented++; }, stopPropagation() {},
    });
    assert.equal(attachments.reading, true);
    await attachments.add([]);
    assert.equal(prevented, 1);
    assert.equal(attachments.count, 1);
    assert.equal(c.att.hidden, false);
    assert.match(c.att.innerHTML, /screen recording.mov/);
    assert.equal(errors.filter(Boolean).length, 0);
    // File chooser selections use the same queue, including repeated picks.
    Object.defineProperty(input, "files", { value: [new File(["clip"], "clip.mp4")], configurable: true });
    input.value = "selected";
    send(input, "change");
    await attachments.add([]);
    assert.equal(attachments.count, 2);
    assert.equal(input.value, "");
    attachments.setBusy(true);
    assert.equal(button.disabled, true);
    await attachments.add([new File(["late"], "late.mov")]);
    assert.equal(attachments.count, 2);
    attachments.setBusy(false);
    const remove = c.att.querySelector<HTMLElement>('[data-startfile="0"]');
    assert.ok(remove);
    send(c.att, "click", { target: remove });
    assert.equal(attachments.count, 1);
    attachments.clear();
    assert.equal(c.att.hidden, true);
  }
});
