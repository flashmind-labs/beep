// A project's terminals: commands kept running on the user's machines,
// apart from any chat. The board's Terminals tile, and its full view: a tab
// per terminal with its output, and a form to start one. center.js draws
// the page; this file keeps the terminals' state and polls the open one.

/// How often the open terminal's output is read again.
const POLL_MS = 2000;

/// "running", "exited 1", "stopped", "offline", "gone".
export function stateLabel(t) {
  if (t.state === "exited") return t.code == null ? "ended" : t.code === 0 ? "done" : `exited ${t.code}`;
  return t.state;
}

/// The board's Terminals tile from the project's terminals; null while
/// they load, when the tile says so.
export function terminalTile(list, ago) {
  if (list === undefined) return { count: "", rows: [], empty: "Loading…" };
  if (list === null) return { count: "", rows: [], empty: "The server keeps no terminals" };
  const running = list.filter(t => t.state === "running").length;
  return {
    count: running ? `${running} running` : list.length ? String(list.length) : "",
    rows: list.map(t => ({
      code: t.name,
      dot: t.state === "running" ? "" : "off",
      note: t.state === "running" ? ago(t.started_at) : stateLabel(t),
      title: `${t.machine_name}: ${t.command} in ${t.cwd}`,
    })),
    empty: "No terminals yet",
  };
}

/// The machines a terminal can start on: those with a checkout, each with
/// its folder.
export function terminalPlaces(where) {
  return (where || []).filter(r => r.kind === "here" && r.folder).map(r => ({ id: r.id, name: r.name, folder: r.folder }));
}

/// `deps`: { invoke, h, button, ago, redraw }.
export function makeTerminals(deps) {
  const { invoke, h, button, ago, redraw } = deps;
  // By project: { list, open, output, form, error, busy }.
  const all = new Map();
  let timer = null;
  let polling = null;

  const of = name => {
    if (!all.has(name)) all.set(name, { list: undefined, open: null, output: null, form: null, error: "", busy: false });
    return all.get(name);
  };
  const fail = e => String(e).replace(/^(bad request|error):\s*/i, "");

  async function load(name) {
    const t = of(name);
    try {
      t.list = await invoke("terminals_list", { project: name });
      if (t.open && !t.list?.some(x => x.id === t.open)) t.open = null;
    } catch (e) {
      t.list ??= [];
      t.error = fail(e);
    }
    redraw();
  }

  async function readOutput(name) {
    const t = of(name);
    if (!t.open) return;
    const id = t.open;
    try {
      const out = await invoke("terminal_output", { id });
      if (t.open !== id) return;
      t.output = out;
      t.list = (t.list || []).map(x => (x.id === id ? out.terminal : x));
      t.error = "";
    } catch (e) {
      t.error = fail(e);
    }
    redraw();
  }

  /// Polls the open terminal of `name` while its view shows; `null` stops.
  function watch(name) {
    if (polling === name) return;
    clearInterval(timer);
    polling = name;
    if (!name) return;
    timer = setInterval(() => {
      if (document.visibilityState === "visible") readOutput(name);
    }, POLL_MS);
  }

  function select(name, id) {
    const t = of(name);
    t.open = id;
    t.form = null;
    t.output = null;
    readOutput(name);
    redraw();
  }

  async function start(name, form) {
    const t = of(name);
    t.busy = true;
    t.error = "";
    redraw();
    try {
      const made = await invoke("terminal_start", { terminal: { project: name, machineId: form.machine, name: form.name, command: form.command, cwd: form.cwd } });
      t.list = [...(t.list || []), made];
      t.form = null;
      select(name, made.id);
    } catch (e) {
      t.error = fail(e);
    } finally {
      t.busy = false;
    }
    redraw();
  }

  async function close(name, id) {
    const t = of(name);
    t.busy = true;
    redraw();
    try {
      await invoke("terminal_close", { id });
      t.list = (t.list || []).filter(x => x.id !== id);
      if (t.open === id) { t.open = null; t.output = null; }
      t.error = "";
    } catch (e) {
      t.error = fail(e);
    } finally {
      t.busy = false;
    }
    redraw();
  }

  function formView(name, places) {
    const t = of(name);
    const f = t.form;
    // A redraw while typing keeps the field and the caret.
    const was = document.activeElement?.closest?.(".tm-form") ? document.activeElement : null;
    const label = was?.getAttribute("aria-label"), caret = was?.selectionStart;
    if (label) queueMicrotask(() => {
      const again = document.querySelector(`.tm-form input[aria-label="${label}"]`);
      again?.focus();
      if (again && caret != null) again.setSelectionRange(caret, caret);
    });
    const input = (key, label, ph) => {
      const el = h("input", "");
      el.value = f[key];
      el.placeholder = ph;
      el.spellcheck = false;
      el.setAttribute("aria-label", label);
      el.addEventListener("input", () => { f[key] = el.value; });
      return h("label", "tm-f", h("span", "", label), el);
    };
    const chips = h("div", "tm-chips", ...places.map(m => {
      const b = button(m.name, "dchip", () => { f.machine = m.id; f.cwd = m.folder; redraw(); });
      b.setAttribute("aria-pressed", String(f.machine === m.id));
      return b;
    }));
    const go = button(t.busy ? "Starting…" : "Start", "pri", () => {
      if (f.command.trim() && f.machine && f.cwd.trim()) start(name, f);
    });
    go.disabled = t.busy || !places.length;
    const form = h("form", "cc-panel tm-form",
      input("command", "Command", "npm run dev"),
      input("name", "Name", "dev-server (the command when empty)"),
      h("div", "tm-f", h("span", "", "Machine"), places.length ? chips : h("span", "dmuted", "No machine has this project yet")),
      input("cwd", "Folder", "The project's folder"),
      h("div", "in-acts", go, button("Cancel", "", () => { t.form = null; redraw(); }),
        h("small", "", "It keeps running when the window closes. Kill stops it.")));
    form.addEventListener("submit", e => { e.preventDefault(); go.click(); });
    return form;
  }

  /// The full view: tabs, then the open terminal's output or the form.
  function panel(name, where) {
    const t = of(name);
    if (t.list === undefined) load(name);
    const places = terminalPlaces(where);
    const tabs = h("div", "in-tabs tm-tabs", ...(t.list || []).map(x => {
      const b = button("", "tm-tab", () => select(name, x.id));
      b.append(h("span", `ddot${x.state === "running" ? "" : " off"}`), x.name);
      if (t.open === x.id) b.setAttribute("aria-current", "true");
      return b;
    }));
    const add = button("+", "tm-tab", () => {
      t.open = null;
      t.form = { command: "", name: "", machine: places[0]?.id || null, cwd: places[0]?.folder || "" };
      redraw();
    });
    add.setAttribute("aria-label", "New terminal");
    add.title = "New terminal";
    tabs.append(add);
    const out = [tabs];
    if (t.error) out.push(h("div", "cc-err", t.error));
    if (t.list === null) {
      out.push(h("div", "cc-panel cc-empty", h("b", "", "The server needs an update before it keeps terminals.")));
      watch(null);
      return out;
    }
    if (t.form) out.push(formView(name, places));
    const open = t.open && (t.list || []).find(x => x.id === t.open);
    if (open) {
      const o = t.output;
      const head = h("div", "tm-head",
        h("span", "", h("b", "", open.name), ` ${open.machine_name} · ${open.cwd}`),
        h("span", "tm-st", stateLabel(open)),
        button(open.state === "running" ? "Kill…" : "Close", "small warn", () => {
          if (open.state !== "running" || confirm(`Stop ${open.name} on ${open.machine_name}?`)) close(name, open.id);
        }));
      const lines = o ? o.lines : [];
      const screen = h("pre", "tm-screen", h("span", "tm-cmd", `${open.machine_name} ${open.cwd} $ ${open.command}`), "\n",
        o?.earlier ? "…\n" : "", lines.join("\n"));
      out.push(h("div", "cc-panel tm-panel", head, screen,
        h("div", "tm-foot", h("span", "", h("span", `ddot${open.state === "running" ? "" : " off"}`), " View only: the last 100 lines, read every 2 seconds"), h("span", "", `started ${ago(open.started_at)}`))));
      watch(name);
    } else {
      watch(null);
      if (!t.form) {
        out.push(h("div", "cc-panel cc-empty", h("b", "", t.list?.length ? "Pick a terminal" : "No terminals yet"),
          h("span", "", "A terminal runs a command on one of your machines, such as a dev server or a log, and keeps running apart from any chat."),
          button("New terminal", "pri", () => add.click())));
      }
    }
    // The screen follows its newest line.
    queueMicrotask(() => { const s = document.querySelector(".tm-screen"); if (s) s.scrollTop = s.scrollHeight; });
    return out;
  }

  return {
    load,
    list: name => of(name).list,
    panel,
    stop: () => watch(null),
  };
}
