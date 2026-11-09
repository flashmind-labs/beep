import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { makeStartComposer } from "../src/agents/start-composer.ts";

test("start composers consume Enter once and preserve Shift+Enter and composition", () => {
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
  assert.equal(composer.input.title, "Enter starts, Shift+Enter adds a line");
  enter({ shiftKey: true });
  enter({ metaKey: true, shiftKey: true });
  enter({ isComposing: true });
  enter({ defaultPrevented: true });
  enter({ metaKey: true, isComposing: true });
  assert.equal(submitted, 0);
  enter();
  assert.equal(submitted, 1);
  assert.equal(consumed, 1, "the window shortcut must not submit the same task again");
  enter({ metaKey: true });
  enter({ ctrlKey: true });
  assert.equal(submitted, 3);
  composer.setPresentation("compact");
  assert.equal(composer.input.title, "Enter starts, Shift+Enter adds a line");
  enter({ shiftKey: true });
  enter({ isComposing: true });
  enter({ defaultPrevented: true });
  assert.equal(submitted, 3);
  enter();
  assert.equal(submitted, 4, "the project page's compact input still starts with Enter");
  assert.equal(consumed, 4);
  assert.equal(composer.input.value, "Sample task");
});
