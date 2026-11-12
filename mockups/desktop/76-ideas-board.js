// ---- The board (#613): what is not built yet ----
// The board itself is in workspace/board.js. These are the parts still proposed: a project filter
// and New agent above Home's lanes, rows by project, a way back to the board with Option-Tab
// to the next conversation on an opened card, and a started task flying from the composer to Working.
{
  const st = document.createElement("style");
  st.textContent = `
.mac .bd-bar { display: flex; align-items: center; gap: 6px; flex: none }
.mac .bd-bar .sp { flex: 1 }
.mac .bd-chip { all: unset; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px; border-radius: 6px; font-size: 12.5px; color: var(--d-muted) }
.mac .bd-chip[aria-pressed=true] { background: var(--d-elev); color: var(--d-fg) }
.mac .bd-chip .cc-pdot { width: 7px; height: 7px; border-radius: 2px }
.mac button.bd-card { all: unset; box-sizing: border-box; cursor: pointer; display: grid; gap: 6px; width: 100%; padding: 10px 12px; border: 1px solid var(--d-border); border-radius: 8px; background: color-mix(in srgb, var(--d-surface) 82%, transparent) }
.mac .bd-card .a { color: #4cb782 } .mac .bd-card .d { color: var(--d-err) }
.mac .bd-rows h4 { margin: 0; display: flex; justify-content: space-between; font-size: 12px; font-weight: 600; color: var(--d-dim) }
.mac .bd-rows h4 span { font-weight: 500; color: var(--d-muted) }
.mac .bd-rows { display: grid; grid-template-columns: 120px repeat(4, minmax(0, 1fr)); column-gap: 12px; align-items: start }
.mac .bd-rows > * { padding: 10px 0; border-top: 1px solid var(--d-border); min-width: 0 }
.mac .bd-rows > .hd { border-top: 0; padding: 0 0 8px }
.mac .bd-rows > .pj { display: flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 500 }
.mac .bd-back { all: unset; cursor: pointer; font-size: 12.5px; color: var(--d-muted) } .mac .bd-back:hover { color: var(--d-fg) }
.mac .bd-next { all: unset; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--d-muted) }
.mac .bd-next kbd { font: 10.5px var(--mono); padding: 1px 5px; border: 1px solid var(--d-border); border-radius: 4px }

::view-transition-old(bd-hero), ::view-transition-new(bd-hero) { height: 100%; object-fit: cover; object-position: left top }
html[data-bddir=next]::view-transition-old(bd-main) { animation: .2s cubic-bezier(.2, 0, 0, 1) both bd-out-up }
html[data-bddir=next]::view-transition-new(bd-main) { animation: .2s cubic-bezier(.2, 0, 0, 1) both bd-in-up }
@keyframes bd-in-up { from { transform: translateY(40px); opacity: 0 } }
@keyframes bd-out-up { to { transform: translateY(-40px); opacity: 0 } }
/* A started task leaves the composer and lands in Working. */
html[data-bddir=lane]::view-transition-group(*) { animation-duration: .32s }
html[data-bddir=lane]::view-transition-old(*), html[data-bddir=lane]::view-transition-new(*) { animation-duration: .32s; height: 100%; object-fit: cover; object-position: left top }
`;
  document.head.append(st);

  const PROJECTS = [["acme/widget", "var(--d-coral)"], ["acme/api", "#6FA8DC"], ["you/site", "#B18CE0"]];
  // [lane, agent, title, project, where, footer, button]
  const CARDS = [
    ["needs", "claude", "May it run xcodebuild on laptop?", "you/site", "laptop · Swipeable approval strip", "", "Answer"],
    ["needs", "", "#12 Move the blog to the new theme", "you/site", "your review", "", "Review"],
    ["needs", "", "#41 Cut the cache warm-up in half", "acme/widget", "sam · checks failing", "", "Reply"],
    ["needs", "", "3 files have two copies", "acme/widget", "build-box, studio", "", "Resolve"],
    ["working", "claude", "Fix the failing login test", "acme/api", "laptop", "4 min", ""],
    ["working", "claude", "Clippy fixes in the sync crate", "acme/api", "build-box", "31 min", ""],
    ["working", "codex", "Fix the device build script", "acme/widget", "studio", "3 min", ""],
    ["review", "codex", "Rewrite the pricing page copy", "you/site", "studio", "diff:88:61", "Review"],
    ["done", "claude", "Retry uploads when the token expires", "acme/widget", "build-box", "diff:212:40", ""],
    ["done", "codex", "Bump the sync crate", "acme/widget", "studio", "diff:18:18", ""],
  ];
  const LANES = [["needs", "Needs you"], ["working", "Working"], ["review", "Ready to review"], ["done", "Done today"]];
  const PROJECT = "acme/widget";
  const DEFAULT_CARD = CARDS.findIndex(c => c[2] === "Fix the failing login test");

  const view = () => S.d.ixb || "all";
  const openCard = () => Number(S.d.ixc ?? DEFAULT_CARD);

  const ag = a => a ? `<span class="cc-ag ${a}">${a === "codex" ? "CX" : "CC"}</span>` : "";
  const foot = text => text.startsWith("diff:") ? `<span class="a">+${text.split(":")[1]}</span><span class="d">−${text.split(":")[2]}</span>` : text;
  // The card last opened carries the shared name, so it grows into the agent header and back.
  // While a lane change plays, every card is named so each one travels on its own.
  const hero = index => S.d.ixlive ? ` style="view-transition-name:bd-card-${index}"` : index === openCard() ? ' style="view-transition-name:bd-hero"' : "";
  const card = (c, showProject) => {
    const [lane, agent, title, project, where, footer, button] = c;
    const index = CARDS.indexOf(c);

    return `<button class="bd-card" data-bdgo="open:${index}"${hero(index)}>
      <b>${title}</b>
      <span class="sub">${ag(agent)}<span>${showProject ? `${project} · ` : ""}${where}</span></span>
      ${footer || button ? `<span class="ft">${foot(footer)}<span class="sp"></span>${button ? `<span class="dbtn small${lane === "needs" && agent ? " pri" : ""}">${button}</span>` : ""}</span>` : ""}
    </button>`;
  };
  const cardsIn = (lane, project) => CARDS.filter(c => c[0] === lane && (!project || c[3] === project));
  const lanes = project => `<div class="bd-lanes">${LANES.map(([key, name]) => {
    const cards = cardsIn(key, project);
    return `<div class="bd-lane"><h4>${name}<span>${cards.length}</span></h4>${cards.map(c => card(c, !project)).join("") || `<div class="bd-empty">Nothing here</div>`}</div>`;
  }).join("")}</div>`;
  const newAgent = `<span class="sp"></span><button class="dbtn small pri" data-dgo="dAgentNew">New agent</button>`;
  const bar = () => `<div class="bd-bar">
    <button class="bd-chip" aria-pressed="true">All projects</button>
    ${PROJECTS.map(([name, color]) => `<button class="bd-chip" aria-pressed="false" data-bdgo="project"><span class="cc-pdot" style="background:${color}"></span>${name}</button>`).join("")}
    ${newAgent}</div>`;
  const rows = () => `<div class="bd-rows"><span class="hd"></span>${LANES.map(([key, name]) => `<h4 class="hd">${name}<span>${cardsIn(key).length}</span></h4>`).join("")}
    ${PROJECTS.map(([name, color]) => `<span class="pj"><span class="cc-pdot" style="background:${color}"></span>${name}</span>${LANES.map(([key]) => `<div class="bd-lane">${cardsIn(key, name).map(c => card(c, false)).join("")}</div>`).join("")}`).join("")}</div>`;

  // The board lives in the real window: a current screen is rendered, then changed in place.
  const frame = (html, current) => {
    const t = document.createElement("template");
    t.innerHTML = html;
    const side = t.content.querySelector(".cc-side");
    side.querySelector('[data-dgo="dAgents"]')?.remove();
    for (const el of side.querySelectorAll('[data-dgo="dWinProject"], [data-dgo="dHome"]:not([data-dact])')) {
      el.dataset.bdgo = el.dataset.dgo === "dHome" ? "all" : "project";
      el.removeAttribute("aria-current");
      delete el.dataset.dgo;
    }
    side.querySelector(current)?.setAttribute("aria-current", "true");
    t.content.querySelector(".cc-main").style.viewTransitionName = "bd-main";
    return t;
  };

  const home = body => {
    const t = frame(D.dHome(), '[data-bdgo="all"]');
    const pane = t.content.querySelector(".cc-view");
    const tabs = pane.querySelector(".cc-tabs");
    tabs.querySelector("button").firstChild.textContent = "Board";
    pane.replaceChildren(pane.querySelector(".cc-mstrip"), tabs);
    pane.insertAdjacentHTML("beforeend", body);
    return t.innerHTML;
  };

  // The project page keeps its header, status strip and composer; the board replaces the list and overview.
  const project = () => {
    const t = frame(D.dWinProject(), ".pj-repo");
    const pane = t.content.querySelector(".cc-view, .bd-pj");
    const strip = pane.querySelector(".pj-strip");
    pane.className = "bd-pj";
    pane.replaceChildren(strip);
    pane.insertAdjacentHTML("beforeend", lanes(PROJECT));
    if (S.d.ixlive === "start") t.content.querySelector(".cc-pjc .box").style.viewTransitionName = `bd-card-${CARDS.length}`;
    return t.innerHTML;
  };

  const opened = () => {
    const [, , title, name, where] = CARDS[openCard()];
    const t = frame(D.dAgentSession(), '[data-bdgo="all"]');
    const top = t.content.querySelector(".cc-top");
    top.querySelector("h2").outerHTML = `<button class="bd-back" data-bdgo="back">‹ Board</button>`;
    top.querySelector("small").outerHTML = `<span class="tools"><button class="bd-next" data-bdgo="next"><kbd>⌥Tab</kbd>next conversation</button></span>`;
    const head = t.content.querySelector(".dhead");
    head.querySelector(".agtitle").textContent = title;
    head.querySelector(".agtitle").dataset.originalTitle = title;
    head.querySelector(".tt small").textContent = `${name} @ ${where.split(" · ")[0]}`;
    head.style.viewTransitionName = "bd-hero";
    return t.innerHTML;
  };

  D.dIdeaBoard = () => {
    const v = view();
    if (v === "open") return opened();
    if (v === "project") return project();
    if (v === "rows") return home(`<div class="bd-bar">${newAgent}</div>${rows()}`);
    return home(bar() + lanes(""));
  };

  // One place moves between views, so every move gets the same transition.
  const DEPTH = { all: 0, rows: 0, project: 1, open: 2 };
  const AGENT_CARDS = CARDS.flatMap((c, i) => c[1] ? [i] : []);
  function move(to) {
    const from = view();
    let direction = "in";
    const change = () => {
      if (to.startsWith("open:")) { S.d.ixfrom = from === "open" ? S.d.ixfrom : from; S.d.ixc = to.split(":")[1]; S.d.ixb = "open"; }
      else if (to === "next") S.d.ixc = AGENT_CARDS[(AGENT_CARDS.indexOf(openCard()) + 1) % AGENT_CARDS.length];
      else S.d.ixb = to === "back" ? S.d.ixfrom || "all" : to;
      render();
    };
    if (to === "next") direction = "next";
    else if (to === "back" || DEPTH[to] < DEPTH[from]) direction = "out";

    const still = matchMedia("(prefers-reduced-motion: reduce)").matches || !document.startViewTransition;
    if (still) return change();
    // The card must carry the shared name before the old view is captured.
    if (to.startsWith("open:")) { S.d.ixc = to.split(":")[1]; if (from !== "open") render(); }
    document.documentElement.dataset.bddir = direction;
    document.startViewTransition(change);
  }

  // The new card is named before the old view is captured, so it travels from the composer.
  function live(kind, change) {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches || !document.startViewTransition) { change(); return render(); }
    S.d.ixlive = kind;
    render();
    document.documentElement.dataset.bddir = "lane";
    const done = () => { S.d.ixlive = ""; render(); };
    document.startViewTransition(() => { change(); if (kind === "start") S.d.ixlive = "lane"; render(); }).finished.then(done, done);
  }
  const start = task => live("start", () => CARDS.push(["working", "claude", task, PROJECT, "build-box", "now", ""]));

  document.addEventListener("click", event => {
    const send = event.target.closest(".bd-pj ~ .cc-pjc .dbtn.pri");
    if (!send || S.screen !== "dIdeaBoard") return;
    event.preventDefault();
    event.stopImmediatePropagation();
    start(send.closest("form").querySelector("textarea").value.trim() || "Bump the SDK to 4.3");
  }, { capture: true });

  document.addEventListener("click", event => {
    const el = event.target.closest("[data-bdgo]");
    if (!el || S.screen !== "dIdeaBoard") return;
    event.preventDefault();
    move(el.dataset.bdgo);
  });
  // Capture on the window so the mock's older Escape and Option-Tab handlers do not also run.
  window.addEventListener("keydown", event => {
    if (S.screen !== "dIdeaBoard" || view() !== "open") return;
    const to = event.altKey && event.key === "Tab" ? "next" : event.key === "Escape" ? "back" : null;
    if (!to) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    move(to);
  }, { capture: true });

  VARIANTS.dIdeaBoard = [
    ...[["Project filter and New agent", "all"], ["Rows by project", "rows"], ["Card opened: back to the board, next conversation", "open"]]
      .map(([label, key]) => [label, () => move(key === "open" ? `open:${openCard()}` : key), () => view() === key]),
    ["Start an agent: the task flies to Working", () => { S.d.ixb = "project"; start("Bump the SDK to 4.3"); }, () => view() === "project"],
  ];

  DESK_SCREENS.unshift({ group: "Ideas (12 Nov)", items: [["dIdeaBoard", "Board: not built yet", "Project filter, rows by project, back to the board, a started task flying to Working"]] });
  if (location.hash.slice(1) === "dIdeaBoard") S.screen = "dIdeaBoard";
  render();
}
