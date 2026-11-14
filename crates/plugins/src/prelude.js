// Evaluates to a function the host calls once with (schedule, canPlace, listen,
// unlisten). These are the only native functions a plugin can reach, through
// the closures below. `schedule(id, ms)`: a negative `ms` cancels the timer; it
// returns false when the host refuses another timer. `listen(id, topicJson)`
// returns "" or why the host refuses the subscription; `unlisten(id)` drops it.
// The function returns { fire(id), emit(eventJson), render() }.
(function (schedule, canPlace, listen, unlisten) {
  "use strict";

  const stringify = JSON.stringify;
  const timers = new Map();
  const placed = [];
  const subscriptions = new Map();
  let nextTimer = 0;
  let nextSubscription = 0;
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

  function optional(value, field) {
    if (value === undefined || value === null) {
      return null;
    }
    if (typeof value !== "string") {
      throw new TypeError(`beep.on needs ${field} to be a string`);
    }
    return value;
  }

  function on(topic, callback) {
    if (topic === null || typeof topic !== "object" || typeof topic.name !== "string") {
      throw new TypeError("beep.on needs a topic with a name");
    }
    if (typeof callback !== "function") {
      throw new TypeError("beep.on needs a callback");
    }
    const wanted = {
      name: topic.name,
      key: optional(topic.key, "key"),
      machine: optional(topic.machine, "machine"),
    };
    const id = ++nextSubscription;
    const refused = listen(id, stringify(wanted));
    if (refused) {
      throw new Error(refused);
    }
    subscriptions.set(id, { topic: wanted, callback });
    return function cancel() {
      if (subscriptions.delete(id)) {
        unlisten(id);
      }
    };
  }

  const image = (props) => Object.assign({ kind: "image" }, props);
  const text = (value) => ({ kind: "text", text: String(value) });
  const row = (options, children) =>
    Object.assign({ kind: "row" }, options, { children });

  const api = { place, state, after, on, image, text, row };
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
    emit(json) {
      const event = JSON.parse(json);
      for (const [id, item] of Array.from(subscriptions)) {
        const topic = item.topic;
        const same =
          topic.name === event.topic.name &&
          topic.key === event.topic.key &&
          topic.machine === event.topic.machine;
        if (same && subscriptions.get(id) === item) {
          item.callback(event);
        }
      }
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
