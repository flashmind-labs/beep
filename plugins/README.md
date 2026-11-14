# Plugins

Prototype. A plugin is a folder `<plugins dir>/<id>/` with a `beep-plugin.json`
and one TypeScript file. The desktop runs it in an embedded sandbox, reloads it
when its files change, and draws what it places. A plugin never touches the DOM.
There are three slots and two permissions (`ui.add`, `sessions.read`).

## Manifest

```json
{ "id": "composer-cat", "name": "Composer cat", "version": "0.1.0",
  "description": "A cat sits on the message box.", "entry": "main.ts", "permissions": ["ui.add"] }
```

- `id`: 2 to 40 chars, lowercase letters, digits and dashes, starts with a
  letter, equals the folder name.
- `version`: semver. `description`: optional. `entry`: optional, default
  `main.ts`, a path inside the folder.
- `permissions`: `ui.add` (place UI) and `sessions.read` (follow `agents` and
  `agent` topics). Unknown permissions or fields fail the load.

## API

Types are in `sdk/beep.d.ts`. The source is one file with no imports, and there
is no DOM, `setTimeout` or `console`.

```ts
/// <reference path="../../sdk/beep.d.ts" />
const frame = beep.state("idle");
beep.place({ slot: "composer", align: "end", offset: 84, sink: 4 }, () =>
  beep.image({ src: `art/cat-${frame.get()}.png`, width: 60, height: 48, pixelated: true }));
beep.place({ slot: "session.row" }, row => beep.text(row.title, { tone: "dim" }));
const cancel = beep.after(140, () => frame.set("blink"));
const stop = beep.on({ name: "ui.composer" }, event => { /* event.data */ });
```

- `place(options, render)`: `render(context)` runs once for each instance of the
  slot on the page, and returns a node, or `null` to draw nothing there. It runs
  again for an instance whose context changed, and for all of them after any
  state change. Instances that did not change are not rendered again.
- `state(initial)`: `get()` and `set(next)`.
- `after(ms, callback)`: one-shot timer, at least 16 ms, returns a cancel function.
- `on(topic, callback)`: follow a topic, see below. Returns a function that stops it.
- `image`, `text`, `row`, `button`, `toggle`, `input`: build nodes. Images are
  paths inside the folder, at most 256 KB.

## Slots

A slot can be on the page many times, and the page tells the host each one with
its own context. `render` gets that context. A slot with no instance is not rendered.
At most 200 instances of a slot are drawn, and a plugin has up to 8 `place` calls.

| Slot | Where | Context |
| --- | --- | --- |
| `composer` | The top edge of the open session's message box. | `{ machine, session }` |
| `session.row` | A session's row in the Coding agents list, between the title and the state. | `{ machine, session, title }` |
| `session.header` | The open session's header, after the title. | `{ machine, session }` |

`composer` stands on an edge: its options are `align` (`start`, `center`, `end`,
default `end`), `offset` and `sink` in px. It takes no pointer except on controls.
The other slots flow inline and take only `slot`; other options fail the load. A
slot draws nothing, and takes no room, until a plugin fills it.

`session.row` takes no controls, since the whole row is a button. Rendering a
button, toggle or input there stops the plugin.

There is no `message` slot yet: the conversation log is windowed, with turn
heights measured by hand, and its rows have no stable message id.

## Nodes

- `text(text, { tone })`: `tone` is `"normal"`, `"dim"` or `"accent"`.
- `row({ gap }, children)`: children left to right.
- `image({ src, width, height, pixelated })`.
- `button({ label, onPress }, children?)`: `label` is the accessible name and the
  content when there are no children.
- `toggle({ label, value, onChange })`: `onChange(value)` gets the new value.
- `input({ label, value, placeholder, onChange, onSubmit })`: one line of text, up
  to 4 KB. `onChange(text)` runs on each change and `onSubmit(text)` on Enter.

Callbacks run on the plugin's thread, then the plugin draws again. Keep an input's
`value` in a state and set it in `onChange`: the page shows what is typed at once
and takes the plugin's `value` when that changes. A callback from an older draw
is ignored. A plugin gets at most 30 events a second. Controls look like the app's
own, are keyboard reachable, and take the pointer only where they are. Using
them needs only `ui.add`.

## Topics

`beep.on({ name, key?, machine? }, callback)` calls `callback(event)` with
`event.topic`, `event.machine` (where it came from) and `event.data`. The
callback is one more entry into the sandbox, followed by the usual redraw.

- `ui.composer`: the session whose message box is shown. `data` is
  `{ machine, session }`, or `null` when none is shown. A plugin that follows it
  late gets the latest value at once. No permission needed. The `composer`
  slot's context says the same, so most plugins need not follow it.
- `agents` and `agent` (a session id as `key`, or all sessions without one):
  the app's session topics. They need `sessions.read`.
- Any other name is refused.

Mesh topic payloads are the app's internal view shapes. They can change between
releases, so narrow `data` and do nothing when it is not what you expect.

Updates queue per plugin and only the newest per topic and machine is delivered.
Events over 256 KB are dropped.

Limits: 16 MB memory, 50 ms of CPU per call (each delivery, event and render
pass too), 64 pending timers, 16 subscriptions, 8 `place` calls, 200 instances
per slot, 64 nodes and 64 KB per drawn instance. Breaking one stops the plugin.

## Try the example

The desktop loads plugins from `plugins/` in its app data folder, or from the
folder named by `BEEP_PLUGINS_DIR`. Phones run none.

```
BEEP_PLUGINS_DIR=$PWD/plugins/examples just desktop-dev
```

`examples/session-notes` puts a note on each session's row, set from an input
and a button in the open session's header.

Regenerate the cat frames with
`python3 -I plugins/examples/composer-cat/art-src/sprite.py`.
