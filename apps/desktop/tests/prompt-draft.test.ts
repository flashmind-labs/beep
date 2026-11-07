import "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";

// Native edits fire input without calling the JavaScript value setter.
class Field extends EventTarget {
  private text = "";
  get value() { return this.text; }
  set value(text: string) { this.text = text; }
  type(text: string) { this.text = text; this.dispatchEvent(new Event("input")); }
  isConnected = false;
  parentElement = null;
  selectionStart = 0;
  selectionEnd = 0;
  classList = { add() {} };
  before() {}
  focus() {}
  setRangeText(text: string, start: number, end: number) {
    this.text = this.text.slice(0, start) + text + this.text.slice(end);
  }
}

Object.assign(globalThis, {
  HTMLTextAreaElement: Field,
  requestAnimationFrame: () => 1,
});
Object.assign(document, {
  createElement: () => ({ setAttribute() {}, className: "", hidden: false }),
  execCommand: () => false,
});
const { composeField, composeMessage } = await import("../src/chat/compose.ts");

const stored = new Map<string, string>();
const storage = {
  getItem: (key: string) => stored.get(key) ?? null,
  setItem: (key: string, text: string) => { stored.set(key, text); },
  removeItem: (key: string) => { stored.delete(key); },
};
Object.assign(globalThis, { localStorage: storage });

function open(draft?: string) {
  const field = new Field();
  composeField(field as unknown as HTMLTextAreaElement, { draft });
  return field;
}

test("typed prompts survive recreating the field with whitespace intact", () => {
  const first = open("session-a");
  first.type("  fix this\n\n\nconst n = 1;\n");
  assert.equal(open("session-a").value, first.value);
  assert.equal(open("session-b").value, "", "other sessions have separate drafts");
});

test("sending clears the draft and a failed send can restore it from code", () => {
  const field = open("send");
  field.type("fix the build");
  field.value = "";
  assert.equal(open("send").value, "");
  field.value = "fix the build";
  assert.equal(open("send").value, "fix the build");
  field.type("");
  assert.equal(open("send").value, "", "deleting text clears the draft too");
});

test("a pasted text pill restores its full text", () => {
  const field = open("paste");
  const text = Array.from({ length: 12 }, (_, i) => `line ${i}`).join("\n");
  const event = new Event("paste", { cancelable: true });
  Object.assign(event, { clipboardData: { items: [], getData: () => text } });
  field.dispatchEvent(event);
  assert.equal(event.defaultPrevented, true);
  assert.notEqual(field.value, text, "the live field holds a pill");
  assert.equal(open("paste").value, text, "the saved draft has no window-local pill IDs");
  assert.equal(composeMessage(open("paste").value), text);
});

test("storage failure leaves typing and clearing usable", () => {
  Object.assign(globalThis, { localStorage: {
    getItem() { throw new Error("unavailable"); },
    setItem() { throw new Error("full"); },
    removeItem() { throw new Error("unavailable"); },
  } });
  try {
    const field = open("offline");
    field.type("keep typing");
    assert.equal(field.value, "keep typing");
    field.value = "";
    assert.equal(field.value, "");
  } finally { Object.assign(globalThis, { localStorage: storage }); }
});

test("composers without a draft key do not save prompts", () => {
  const before = stored.size;
  open().type("temporary");
  assert.equal(stored.size, before);
});
