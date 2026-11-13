// Evaluates to a function the host calls once with (schedule, canPlace).
// `schedule(id, ms)` is the only native function a plugin can reach, through
// the closures below. A negative `ms` cancels the timer. It returns false when
// the host refuses another timer. The function returns { fire(id), render() }.
(function (schedule, canPlace) {
  "use strict";

  const stringify = JSON.stringify;
  const timers = new Map();
  const placed = [];
  let nextTimer = 0;
  let dirty = true;

  function place(options, render) {
    if (!canPlace) {
      throw new Error("beep.place needs the ui.add permission");
    }
    if (typeof render !== "function") {
      throw new TypeError("beep.place needs a render function");
    }
    placed.push({ options: Object.assign({}, options), render });
    dirty = true;
  }

  function state(initial) {
    let value = initial;
    return {
      get() {
        return value;
      },
      set(next) {
        value = next;
        dirty = true;
      },
    };
  }

  function after(ms, callback) {
    if (typeof callback !== "function") {
      throw new TypeError("beep.after needs a callback");
    }
    const delay = Number.isFinite(+ms) ? Math.max(0, +ms) : 0;
    const id = ++nextTimer;
    if (!schedule(id, delay)) {
      throw new Error("too many pending timers");
    }
    timers.set(id, callback);
    return function cancel() {
      if (timers.delete(id)) {
        schedule(id, -1);
      }
    };
  }

  const image = (props) => Object.assign({ kind: "image" }, props);
  const text = (value) => ({ kind: "text", text: String(value) });
  const row = (options, children) =>
    Object.assign({ kind: "row" }, options, { children });

  const api = { place, state, after, image, text, row };
  Object.defineProperty(globalThis, "beep", {
    value: Object.freeze(api),
    writable: false,
    configurable: false,
    enumerable: false,
  });

  return {
    fire(id) {
      const callback = timers.get(id);
      if (callback === undefined) {
        return;
      }
      timers.delete(id);
      callback();
    },
    render() {
      if (!dirty) {
        return null;
      }
      dirty = false;
      return stringify(
        placed.map((item) => Object.assign({}, item.options, { node: item.render() })),
      );
    },
  };
})
