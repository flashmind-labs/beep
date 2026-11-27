import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { startDom } from "./start-react-env.ts";

test("both start layouts accept file drops and protect pending files while starting", async () => {
  const env = startDom();
  try {
    const { makeStartComposer } = await import("../src/parts/session/start-composer.tsx");
    const { startAttachments } = await import("../src/parts/session/start-attachments.tsx");
    for (const presentation of ["compact", "centered"] as const) {
      const c = makeStartComposer({ place: () => null, presentation, actions: { submit() {}, harness() {}, files: files => attachments.take(files) } }); env.dom.window.document.body.append(c.form);
      const errors: string[] = [];
      const attachments = startAttachments(c, error => errors.push(error), () => {});
      const input = c.form.querySelector<HTMLInputElement>('input[type="file"]'); assert.ok(input);
      assert.equal(input.multiple, true); assert.equal(input.hidden, true);
      const button = c.form.querySelector<HTMLButtonElement>('[aria-label="Attach files"]'); assert.ok(button);
      assert.equal(button.type, "button");
      let picked = 0; input.click = () => { picked++; }; button.click(); assert.equal(picked, 1);
      const drop = new env.dom.window.Event("drop", { bubbles: true, cancelable: true });
      Object.defineProperty(drop, "dataTransfer", { value: { files: [new File(["video"], "screen recording.mov", { type: "video/quicktime" })] } });
      c.form.dispatchEvent(drop); assert.equal(drop.defaultPrevented, true);
      await attachments.add([]);
      assert.equal(attachments.count, 1); assert.equal(c.att.hidden, false); assert.match(c.att.innerHTML, /1 file/);
      const firstChip = c.att.firstElementChild;
      assert.equal(errors.filter(Boolean).length, 0);
      Object.defineProperty(input, "files", { value: [new File(["clip"], "clip.mp4")], configurable: true });
      input.dispatchEvent(new env.dom.window.Event("change", { bubbles: true })); await attachments.add([]);
      assert.equal(attachments.count, 2); assert.equal(input.value, "");
      assert.equal(c.att.firstElementChild, firstChip, "adding a file keeps existing attachment controls");
      attachments.setBusy(true); assert.equal(button.disabled, true);
      await attachments.add([new File(["late"], "late.mov")]); assert.equal(attachments.count, 2);
      attachments.setBusy(false);
      c.setBusy(true); c.setBusy(false);
      c.setPresentation(presentation === "compact" ? "centered" : "compact");
      assert.equal(c.att.firstElementChild, firstChip, "presentation updates keep attachment controls");
      assert.equal(c.form.querySelector('[aria-label="Attach files"]'), button);
      assert.equal(attachments.count, 2); assert.equal(c.att.hidden, false);
      attachments.removeLast();
      assert.equal(attachments.count, 1); attachments.clear(); assert.equal(c.att.hidden, true);
      c.dispose(); c.form.remove();
    }
  } finally { env.close(); }
});
