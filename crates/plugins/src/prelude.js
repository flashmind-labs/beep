// Evaluates to a function the host calls once with (schedule, canPlace, listen,
// unlisten, checkPlace). These are the only native functions a plugin can reach, through
// the closures below. `schedule(id, ms)`: a negative `ms` cancels the timer; it
// returns false when the host refuses another timer. `listen(id, topicJson)`
// returns "" or why the host refuses the subscription; `unlisten(id)` drops it.
// `checkPlace(optionsJson)` returns "" or why the host refuses the options.
// The function returns { fire(id), emit(eventJson), instances(json),
// invoke(id, argumentJson), render() }.
//
// A slot can be on the page many times. The host tells the prelude which
// instances are live (`instances`), and `render()` runs each placement once per
// live instance of its slot, with that instance's context. It returns one JSON
// string per placement and instance, or null when nothing changed. An entry
// is drawn again only when its context changed or a state was set.
(function (schedule, canPlace, listen, unlisten, checkPlace) {
  "use strict";

  const stringify = JSON.stringify;
  const MAX_PLACEMENTS = 8;
  const MAX_DEPTH = 16;
  const timers = new Map();
  const placed = [];
  const subscriptions = new Map();
  // Callbacks in trees now shown, by id: { fn, accepts }. Ids are never reused.
  const handlers = new Map();
  // Live instances by "slot\n key": { slot, key, context, json }.
  let live = new Map();
  let liveJson = "";
  let nextTimer = 0;
  let nextSubscription = 0;
  let nextHandler = 0;
  // Counts state changes. An entry drawn at another count is drawn again.
  let version = 0;
  let dirty = true;

  function place(options, render) {
    if (!canPlace) {
      throw new Error("beep.place needs the ui.add permission");
    }
    if (typeof render !== "function") {
      throw new TypeError("beep.place needs a render function");
    }
    const refused = checkPlace(stringify(options));
    if (refused) {
      throw new Error(refused);
    }
    if (placed.length >= MAX_PLACEMENTS) {
      throw new Error(`at most ${MAX_PLACEMENTS} placements`);
    }
    placed.push({ options: Object.assign({}, options), render, entries: new Map() });
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
        version += 1;
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
  const sprite = (props) => Object.assign({ kind: "sprite" }, props);
  const text = (value, options) =>
    Object.assign({ kind: "text" }, options, { text: String(value) });
  const row = (options, children) =>
    Object.assign({ kind: "row" }, options, { children });
  const button = (options, children) =>
    Object.assign({ kind: "button" }, options, { children: children || [] });
  const toggle = (options) => Object.assign({ kind: "toggle" }, options);
  const input = (options) => Object.assign({ kind: "input" }, options);

  // The callback properties of each kind, and what the callback accepts.
  const CALLBACKS = {
    button: { onPress: { accepts: "null", required: true } },
    toggle: { onChange: { accepts: "boolean", required: true } },
    input: {
      onChange: { accepts: "string", required: false },
      onSubmit: { accepts: "string", required: false },
    },
  };

  // Copies a tree, replacing each callback with an id the page can send back.
  function encode(node, ids, depth) {
    if (node === null || typeof node !== "object" || depth > MAX_DEPTH) {
      return node;
    }
    const callbacks = CALLBACKS[node.kind] || {};
    const copy = {};
    for (const key of Object.keys(node)) {
      const value = node[key];
      const spec = callbacks[key];
      if (spec !== undefined) {
        if (typeof value === "function") {
          const id = ++nextHandler;
          handlers.set(id, { fn: value, accepts: spec.accepts });
          ids.push(id);
          copy[key] = id;
        } else if (value !== undefined && value !== null) {
          throw new TypeError(`${node.kind} ${key} must be a function`);
        }
      } else if (key === "children" && Array.isArray(value)) {
        copy.children = value.map((child) => encode(child, ids, depth + 1));
      } else {
        copy[key] = value;
      }
    }
    for (const [key, spec] of Object.entries(callbacks)) {
      if (spec.required && !(key in copy)) {
        throw new TypeError(`${node.kind} needs ${key}`);
      }
    }
    return copy;
  }

  function drop(entry) {
    if (entry !== undefined) {
      for (const id of entry.ids) {
        handlers.delete(id);
      }
    }
  }

  function draw(item, instance) {
    const ids = [];
    let text;
    try {
      const tree = item.render(JSON.parse(instance.json));
      // A render that returns nothing draws nothing for this instance.
      text =
        tree === null || tree === undefined
          ? null
          : stringify({
              instance: instance.key,
              place: item.options,
              node: encode(tree, ids, 0),
            });
    } catch (error) {
      for (const id of ids) {
        handlers.delete(id);
      }
      throw error;
    }
    return { version, json: instance.json, ids, text };
  }

  const api = { place, state, after, on, image, sprite, text, row, button, toggle, input };
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
    instances(json) {
      if (json === liveJson) {
        return;
      }
      liveJson = json;
      live = new Map();
      for (const item of JSON.parse(json)) {
        live.set(`${item.slot}\n${item.key}`, {
          slot: item.slot,
          key: item.key,
          json: stringify(item.context),
        });
      }
      dirty = true;
    },
    invoke(id, argument) {
      const handler = handlers.get(id);
      if (handler === undefined) {
        return false;
      }
      const value = JSON.parse(argument);
      const type = value === null ? "null" : typeof value;
      if (type !== handler.accepts) {
        return false;
      }
      if (value === null) {
        handler.fn();
      } else {
        handler.fn(value);
      }
      return true;
    },
    render() {
      if (!dirty) {
        return null;
      }
      dirty = false;
      const out = [];
      for (const item of placed) {
        const slot = item.options.slot;
        for (const [id, entry] of item.entries) {
          if (!live.has(id)) {
            drop(entry);
            item.entries.delete(id);
          }
        }
        for (const [id, instance] of live) {
          if (instance.slot !== slot) {
            continue;
          }
          let entry = item.entries.get(id);
          if (entry === undefined || entry.version !== version || entry.json !== instance.json) {
            drop(entry);
            item.entries.delete(id);
            entry = draw(item, instance);
            item.entries.set(id, entry);
          }
          if (entry.text !== null) {
            out.push(entry.text);
          }
        }
      }
      return out;
    },
  };
})
