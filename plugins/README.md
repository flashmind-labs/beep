# Plugins

Prototype. A plugin is a folder `<plugins dir>/<id>/` with a `beep-plugin.json`
and one TypeScript file. The desktop runs it in an embedded sandbox, reloads it
when its files change, and draws what it places. A plugin never touches the DOM.
There are three slots and two permissions (`ui.add`, `sessions.read`).
Folders and files whose names start with a dot are never plugins.

## Installing and approval

Settings, Plugins lists every plugin with its source, permissions and state, and
turns each on or off. Install one from Git (`owner/repo`, or an https or ssh
address, with an optional branch, tag or commit and an optional folder of the
repository) or from a folder on the computer. Beep fetches it into a staging
folder, shows the plugin, its source and commit and what it asks for, and installs
only when you choose Allow and install. Cancel deletes the staging folder. A Git
plugin's Update looks for a newer commit on its branch and shows the permissions
it adds or drops before anything changes. Beep never updates a plugin on its own.
Installs are capped at 16 MB and refuse symlinks that leave the plugin folder.
Git needs to be installed; it runs with no prompts, no hooks and no submodules.

Beep keeps one record per plugin in `<plugins dir>/.plugins.json`: whether it is
on, the permissions you allowed and where it came from. A plugin runs only when
it is on and every permission its manifest asks for is allowed. A manifest that
adds a permission, by an update or an edit, puts the plugin back to Needs approval
and it stops until you allow it in Settings. A broken or missing record file is
logged and read as empty.

A folder in the app data plugins dir with no record waits for approval. When
the plugins dir is named by `BEEP_PLUGINS_DIR` (development), a plugin with no
record counts as on and allowed, and shows as a Dev folder. Turning it on or off,
installing or updating it makes a record, and the record rules apply from then on.

## Manifest

```json
{ "id": "composer-cat", "name": "Composer cat", "version": "0.2.0",
  "description": "A cat sits on the message box.", "entry": "main.ts", "permissions": ["ui.add"],
  "activation": ["slot:composer"] }
```

- `id`: 2 to 40 chars, lowercase letters, digits and dashes, starts with a
  letter, equals the folder name.
- `version`: semver. `description`: optional. `entry`: optional, default
  `main.ts`, a path inside the folder.
- `activation`: optional, when the plugin starts, default `["startup"]`. See
  Activation.
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
  Timers wait while no window is visible.
- `on(topic, callback)`: follow a topic, see below. Returns a function that stops it.
- `image`, `sprite`, `text`, `row`, `button`, `toggle`, `input`: build nodes. Images
  are paths inside the folder, at most 256 KB each and 4 MB per plugin.

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
- `sprite({ rest, width, height, pixelated, motions })`: see Animation.
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

## Animation

A plugin never animates by timers: that wakes its thread, runs its script and sends
a new tree to the page for every frame. Declare the animation with `sprite` and the
page plays it, with no wakeups of the plugin and no traffic.

```ts
beep.sprite({
  rest: "art/cat-idle.png", width: 72, height: 56, pixelated: true,
  motions: [
    { steps: [{ frame: "art/cat-blink.png", ms: 140 }], repeat: { every: { min: 3000, max: 7000 } } },
  ],
});
```

The sprite shows `rest`. Each motion is a list of steps (a frame path and how long
it shows, 16 to 60000 ms), and after the last step the sprite goes back to `rest`.
`repeat` says when a motion plays:

- `"once"` (default): once, when the sprite appears.
- `{ every: { min, max } }`: after a random delay between `min` and `max` ms (100 to
  3600000), and again after each play with a new delay.
- `"loop"`: again and again with no rest. It must be the only motion.

Motions play one at a time, in due order. A sprite has at most 8 motions, 64 steps
in each and 32 distinct frames. To change what the sprite does, return another node
after a state change (the cat returns a plain `image` while it is happy). The page
draws only the rest frame while its window is hidden or the user asks for reduced
motion, and starts the motions again when it can.

Images are sent to the page once per content hash, not in every update, and the page
drops one when no placement uses it.

## Activation

`activation` lists what starts the plugin, and any one is enough:

- `"startup"`: when the app starts. The plugin runs until the app quits. Default.
- `"slot:<slot>"`: when the page first shows an instance of that slot.
- `"topic:ui.<name>"`: when the page first publishes that `ui.` topic.

The host reads the manifest without running the plugin, so a plugin that waits costs
nothing. A plugin whose activation is only slots stops when none of those slots has
been on the page for 60 seconds, and starts again when one returns, with its state
reset. List every slot the plugin draws in. A plugin that lists `startup` or a topic
keeps running once started.

## Hidden windows and cost

The page tells the host when its windows are visible. While none is, plugins are
paused: nothing is drawn, timers wait (overdue ones run once each when a window shows
again), and topic updates pile up, only the newest per topic and machine kept, and
arrive then. A plugin cannot run while hidden.

The host adds up the time a plugin spends in its script. A plugin that uses more than
500 ms in any 60 seconds is stopped, with the reason, and shows nothing until its files
change. The `plugins_status` command lists each plugin's id, name, version, description,
`permissions` asked for, `approved` permissions, `source` (`local`, `git` or `dev`),
`enabled`, `state` (`waiting`, `running`, `paused`, `disabled`, `needs-approval` with
the `missing` permissions, or `stopped` with a `reason`), `usedMs` over the last minute
and `pendingTimers`. The desktop emits `plugins-status` when a plugin is added, removed
or changes state.

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
pass too) and 500 ms per minute, 64 pending timers, 16 subscriptions, 8 `place` calls, 200 instances
per slot, 64 nodes and 64 KB per drawn instance. Breaking one stops the plugin.

## Try the example

The desktop loads plugins from `plugins/` in its app data folder, or from the
folder named by `BEEP_PLUGINS_DIR`, which runs plugins without approval records, as
described above. Phones run none.

```
BEEP_PLUGINS_DIR=$PWD/plugins/examples just desktop-dev
```

`examples/session-notes` puts a note on each session's row, set from an input
and a button in the open session's header.

Regenerate the cat frames with
`python3 -I plugins/examples/composer-cat/art-src/sprite.py`.
