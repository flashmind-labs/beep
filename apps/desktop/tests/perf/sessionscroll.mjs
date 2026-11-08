// Manual check that a coding agent session opens at the end of its log and
// stays there while its reads come in (#439). Same environment as check.mjs.
//
//   PLAYWRIGHT_DIR=/path/with/playwright/ node tests/perf/sessionscroll.mjs [--browser webkit|chromium] [--dist DIR] [--flow open,reopen,switch,restore,restore-up] [--trace]
//
// The session has 1002 entries: the first read gets the newest 200, whose
// oldest turn lacks its prompt, so a read of older entries follows. Every
// answer comes after a delay, as from a machine over the mesh. Each way of
// opening runs in its own page (`--flow open,reopen,switch`).
import { join, normalize } from "node:path";
import { fileURLToPath } from "node:url";
import { makeData } from "./data.mjs";
import { playwright, serve, stubInit } from "./stub.mjs";

const args = process.argv.slice(2);
const opt = (name, fallback) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1] : fallback; };
const here = fileURLToPath(new URL(".", import.meta.url));
const { server, origin } = await serve(normalize(opt("dist", join(here, "../../dist"))));
const { webkit, chromium } = playwright();
const kind = opt("browser", "webkit");
const executablePath = (kind === "webkit" ? process.env.WEBKIT_BIN : process.env.CHROMIUM_BIN) || undefined;
const browser = await (kind === "webkit" ? webkit : chromium).launch({ executablePath });
const sleep = ms => new Promise(r => setTimeout(r, ms));

const SESSION = {
  kind: "agent", machine_id: "fake-machine-00", machine_name: "fake-box-00", id: "fake-session-scroll", agent: "claude",
  title: "Fake long session", cwd: "/fake/work/fake-repo-000", status: "idle", doing: null,
  started_at: 0, last_activity: 0, external: false,
};

/// A log of `n` entries in turns: a prompt, thoughts, tools with long
/// output, an answer with a code block, now and then a picture.
function entries(n) {
  const out = [];
  for (let t = 0; out.length < n; t++) {
    out.push({ kind: "user", text: `Fake prompt ${t}: change the widget so it does thing ${t}.` });
    out.push({ kind: "thought", text: `**Looking at widget ${t}**\nIt reads the config first.`, html: `<p><strong>Looking at widget ${t}</strong></p>`, ms: 1200 });
    for (let k = 0; k < 2; k++) {
      const lines = Array.from({ length: 40 + (t % 5) * 20 }, (_, i) => `line ${i} of fake output for step ${k} in turn ${t}`).join("\n");
      out.push({ kind: "tool", text: `Run \`cargo test -p fake-${t}\` [completed]\n\`\`\`\n${lines}\n\`\`\`` });
    }
    const code = Array.from({ length: 6 + (t % 7) * 3 }, (_, i) => `fn fake_${t}_${i}() -> u32 { ${i} }`).join("\n");
    const pic = t % 2 === 1 ? `\n[image: image-${t.toString(16).padStart(16, "0")}]` : "";
    out.push({ kind: "agent", text: `Done with turn ${t}.\n\n\`\`\`rust\n${code}\n\`\`\`${pic}`, html: `<p>Done with turn ${t}. ${"Some explanation of the change. ".repeat(1 + t % 6)}</p><pre><code class="language-rust">${code}</code></pre>` });
  }
  return out.slice(0, n);
}

const LOG = entries(1002);
// The newest 200 start mid-turn, so the first read lacks its prompt.
if (LOG[LOG.length - 200].kind === "user") throw new Error("the first page must start mid-turn");

const delays = { view: Number(opt("view-ms", "350")), older: Number(opt("older-ms", "500")), thumb: Number(opt("thumb-ms", "700")) };

/// Runs in the page after stubInit: answers the session's reads from `log`, late.
const sessionStub = ([log, session, delays]) => {
  const inner = window.__TAURI__.core.invoke;
  const later = (ms, v) => new Promise(r => setTimeout(() => r(v), ms));
  const page = (id, last, before) => {
    const end = before ?? log.length, start = Math.max(0, end - last);
    return { session: { ...session, id }, plan: [], files: [], last_message: "", error: null, queued: 0, turns: 0, entries: log.slice(start, end), earlier: start };
  };
  // A picture that loads late.
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="600" height="420"><rect width="600" height="420" fill="#7a9"/></svg>`;
  window.__TAURI__.core.invoke = async (cmd, a = {}) => {
    if (cmd === "editors") return [];
    if (cmd === "agent_view") return later(a.before == null ? delays.view : delays.older, page(a.session, a.last, a.before));
    if (cmd === "agent_thumb" || cmd === "agent_image") return later(delays.thumb, `data:image/svg+xml,${encodeURIComponent(svg)}`);
    if (cmd === "agent_modes") return later(delays.view, null);
    if (cmd === "agent_tasks") return later(delays.view, []);
    if (cmd === "agent_usage") return later(delays.view, null);
    return inner(cmd, a);
  };
};

let failed = 0;
const check = (name, ok, detail) => { console.log(`${ok ? "ok  " : "FAIL"} ${name}${detail ? ` (${detail})` : ""}`); if (!ok) failed++; };
const gap = r => r.height - r.top - r.client;

// `open`: a first open. `reopen`: the session was open at its end before,
// and Home showed since. `switch`: another session was open, scrolled up.
// `restore` and `restore-up`: a hidden window resets its log before showing.
for (const flow of (opt("flow", "") || "open,reopen,switch,restore,restore-up").split(",")) {
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
  page.on("pageerror", error => { console.error(error.stack); failed++; });
  await page.addInitScript(stubInit, JSON.stringify(makeData()));
  await page.addInitScript(sessionStub, [LOG, SESSION, delays]);
  await page.goto(`${origin}/center.html`);
  await page.waitForSelector("#side");
  await sleep(500);
  const openSession = s => page.evaluate(s => { window.__pending = { agent: s }; window.__emit("center-open"); }, s);
  let keptTop;
  if (flow !== "open") {
    await openSession(flow === "switch" ? { ...SESSION, id: "fake-session-other" } : SESSION);
    await sleep(4000);
    if (flow === "restore-up") {
      await page.evaluate(() => { const b = document.querySelector("#agbody"); b.scrollTop = b.scrollHeight / 2; });
      await sleep(500);
      keptTop = await page.locator("#agbody").evaluate(b => b.scrollTop);
    }
    if (flow === "switch") await page.evaluate(() => { const b = document.querySelector("#agbody"); b.scrollTop = b.scrollHeight / 2; });
    else if (flow.startsWith("restore")) await page.evaluate(() => {
      window.__emit("center-hidden");
      // WebKit can reset the scroll of a hidden view while it stays connected.
      document.querySelector("#agbody").scrollTop = 0;
    });
    else await page.evaluate(() => { window.__pending = "home"; window.__emit("center-open"); });
    await sleep(500);
  }
  // Open the session as the sidebar or Home does, and record the log every frame for 3 s.
  const trace = await page.evaluate(([session, flow]) => new Promise(done => {
    const out = [], t0 = performance.now();
    const tick = () => {
      const b = document.querySelector("#agbody");
      if (b) out.push({ t: Math.round(performance.now() - t0), top: Math.round(b.scrollTop), height: b.scrollHeight, client: b.clientHeight, stuck: b.stuck, turns: b.querySelectorAll(".agturn").length, pics: b.querySelectorAll("img.pic[src]").length });
      if (performance.now() - t0 < 3000) requestAnimationFrame(tick); else done(out);
    };
    if (flow.startsWith("restore")) window.__emit("center-shown");
    else {
      window.__pending = { agent: session };
      window.__emit("center-open");
    }
    requestAnimationFrame(tick);
  }), [SESSION, flow]);

  // With --trace, every frame where the log changed.
  if (args.includes("--trace")) {
    let last = "";
    for (const { t, ...r } of trace) { const now = JSON.stringify(r); if (now !== last) console.log(flow, t, now); last = now; }
  }
  const end = trace.at(-1), off = trace.filter(r => r.turns && gap(r) > 40);
  check(`${flow}: the session's log drew`, !!end && end.turns > 0, end && `${end.turns} turns`);
  if (flow === "restore-up") {
    check(`${flow}: it keeps the reader's position`, !!end && Math.abs(end.top - keptTop) <= 1 && end.stuck === false, end && JSON.stringify({ ...end, keptTop }));
  } else {
    check(`${flow}: it ends at the bottom and follows it`, !!end && gap(end) <= 40 && end.stuck !== false, end && JSON.stringify(end));
    check(`${flow}: no frame after the first draw is left off the bottom`, off.length <= 2, off.length ? `${off.length} frames, first ${JSON.stringify(off[0])}` : "");
  }
  await page.close();
}

await browser.close();
server.close();
console.log(failed ? `${failed} failed` : "all ok");
process.exit(failed ? 1 : 0);
