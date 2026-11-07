import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeStartComposer } from "../src/agents/start-composer.ts";

test("a centered start consumes Command+Enter once and leaves multiline input alone", () => {
  const proto = Object.getPrototypeOf(document.createElement("textarea"));
  const listen = proto.addEventListener;
  let key: ((event: KeyboardEvent) => void) | undefined;
  proto.addEventListener = function(this: HTMLElement, name: string, listener: (event: KeyboardEvent) => void) {
    if (this.tagName === "textarea" && name === "keydown") key = listener;
  };
  let composer: ReturnType<typeof makeStartComposer>;
  try { composer = makeStartComposer(); }
  finally { proto.addEventListener = listen; }
  let submitted = 0, consumed = 0;
  composer.form.requestSubmit = () => { submitted++; };
  composer.input.value = "Sample task";
  const enter = (options: Partial<KeyboardEvent> = {}) => key!({
    key: "Enter", shiftKey: false, metaKey: false, ctrlKey: false, isComposing: false, defaultPrevented: false,
    preventDefault() {}, stopPropagation() { consumed++; }, ...options,
  } as KeyboardEvent);
  enter();
  enter({ metaKey: true, shiftKey: true });
  enter({ metaKey: true, isComposing: true });
  assert.equal(submitted, 0);
  enter({ metaKey: true });
  assert.equal(submitted, 1);
  assert.equal(consumed, 1, "the window shortcut must not submit the same task again");
  enter({ ctrlKey: true });
  assert.equal(submitted, 2);
  composer.setPresentation("compact");
  enter();
  assert.equal(submitted, 3, "the project page's compact input still starts with Enter");
  assert.equal(composer.input.value, "Sample task");
});
