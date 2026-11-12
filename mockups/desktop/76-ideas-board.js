// ---- The board (#613): Home's first tab and a project's front page ----
// Needs you, Working, Ready to review and Done replace Home's Overview, the Running page and
// the project page. A card opens today's agent view as it is; Option-Tab moves between
// conversations. Opening and going back morph the card into the agent header (picked 12 Nov).
// A card that changes lane travels there. A started task flying from the composer to Working
// is a proposal: the app opens the new session instead.
{
  const st = document.createElement("style");
  st.textContent = `
.mac .bd-bar { display: flex; align-items: center; gap: 6px; flex: none }
.mac .bd-bar .sp { flex: 1 }
.mac .bd-chip { all: unset; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px; border-radius: 6px; font-size: 12.5px; color: var(--d-muted) }
.mac .bd-chip[aria-pressed=true] { background: var(--d-elev); color: var(--d-fg) }
.mac .bd-chip .cc-pdot { width: 7px; height: 7px; border-radius: 2px }
.mac .bd-lanes { flex: 1; min-height: 0; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; align-items: start }
.mac .bd-lane { display: grid; gap: 8px; min-width: 0 }
.mac .bd-lane > h4, .mac .bd-rows h4 { margin: 0; display: flex; justify-content: space-between; font-size: 12px; font-weight: 600; color: var(--d-dim) }
.mac .bd-lane > h4 span, .mac .bd-rows h4 span { font-weight: 500; color: var(--d-muted) }
.mac .bd-card { all: unset; box-sizing: border-box; cursor: pointer; display: grid; gap: 6px; width: 100%; padding: 10px 12px; border: 1px solid var(--d-border); border-radius: 8px; background: color-mix(in srgb, var(--d-surface) 82%, transparent) }
.mac .bd-card:hover { border-color: var(--d-muted) }
.mac .bd-card b { font-weight: 500; font-size: 13px; line-height: 18px }
.mac .bd-card .sub { display: flex; align-items: center; gap: 6px; font-size: 11.5px; color: var(--d-muted); min-width: 0; white-space: nowrap; overflow: hidden }
.mac .bd-card .sub .cc-ag { width: 18px; height: 18px; border-radius: 5px; font-size: 8px; flex: none }
.mac .bd-card .ft { display: flex; align-items: center; gap: 8px; font-size: 11.5px; color: var(--d-muted); font-variant-numeric: tabular-nums }
.mac .bd-card .ft .sp { flex: 1 }
.mac .bd-card .a { color: #4cb782 } .mac .bd-card .d { color: var(--d-err) }
.mac .bd-empty { padding: 10px 12px; border: 1px dashed var(--d-border); border-radius: 8px; font-size: 11.5px; color: var(--d-muted) }
.mac .bd-rows { display: grid; grid-template-columns: 120px repeat(4, minmax(0, 1fr)); column-gap: 12px; align-items: start }
.mac .bd-rows > * { padding: 10px 0; border-top: 1px solid var(--d-border); min-width: 0 }
.mac .bd-rows > .hd { border-top: 0; padding: 0 0 8px }
.mac .bd-rows > .pj { display: flex; align-items: center; gap: 7px; font-size: 13px; font-weight: 500 }
.mac .bd-back { all: unset; cursor: pointer; font-size: 12.5px; color: var(--d-muted) } .mac .bd-back:hover { color: var(--d-fg) }
.mac .bd-pj { flex: 1; min-height: 0; overflow: auto; padding: 14px 16px; display: flex; flex-direction: column; gap: 14px; scrollbar-width: thin }
.mac .bd-pj .cc-mstrip { margin: 0; flex: none }
.mac .bd-next { all: unset; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--d-muted) }
.mac .bd-next kbd { font: 10.5px var(--mono); padding: 1px 5px; border: 1px solid var(--d-border); border-radius: 4px }

/* Navigation motion: one short transition per move, nothing ambient. The sidebar never moves. */
::view-transition-group(*) { animation-duration: .2s; animation-timing-function: cubic-bezier(.2, 0, 0, 1) }
::view-transition-old(*), ::view-transition-new(*) { animation-duration: .2s; animation-timing-function: cubic-bezier(.2, 0, 0, 1) }
::view-transition-old(root), ::view-transition-new(root) { animation: none }
::view-transition-old(bd-hero), ::view-transition-new(bd-hero) { height: 100%; object-fit: cover; object-position: left top }
html[data-bddir=next]::view-transition-old(bd-main) { animation: .2s cubic-bezier(.2, 0, 0, 1) both bd-out-up }
html[data-bddir=next]::view-transition-new(bd-main) { animation: .2s cubic-bezier(.2, 0, 0, 1) both bd-in-up }
@keyframes bd-in-up { from { transform: translateY(40px); opacity: 0 } }
@keyframes bd-out-up { to { transform: translateY(-40px); opacity: 0 } }
/* A card that changes lane travels there; a started task leaves the composer and lands in Working. */
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
      <span class="sub">${ag(agent)}${showProject ? `${project} · ` : ""}${where}</span>
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
    const pane = t.content.querySelector(".cc-view");
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

  // Lane changes come from agents, not from a click, so the app would batch them and skip them when hidden.
  function live(kind, change) {
    if (matchMedia("(prefers-reduced-motion: reduce)").matches || !document.startViewTransition) { change(); return render(); }
    S.d.ixlive = kind;
    render();
    document.documentElement.dataset.bddir = "lane";
    const done = () => { S.d.ixlive = ""; render(); };
    document.startViewTransition(() => { change(); if (kind === "start") S.d.ixlive = "lane"; render(); }).finished.then(done, done);
  }
  const ASKING = CARDS.find(c => c[2] === "Clippy fixes in the sync crate");
  const laneChange = () => live("lane", () => {
    const asks = ASKING[0] === "working";
    ASKING.splice(0, 1, asks ? "needs" : "working");
    ASKING.splice(5, 2, asks ? "" : "31 min", asks ? "Answer" : "");
  });
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
    ...[["Board", "all"], ["Project board", "project"], ["Rows by project", "rows"], ["Card opened", "open"]]
      .map(([label, key]) => [label, () => move(key === "open" ? `open:${openCard()}` : key), () => view() === key]),
    ["Play: a card changes lane", () => { if (view() === "open" || view() === "project") S.d.ixb = "all"; laneChange(); }, () => false],
    ["Proposal: start an agent", () => { S.d.ixb = "project"; start("Bump the SDK to 4.3"); }, () => false],
  ];

  DESK_SCREENS.unshift({ group: "Ideas (12 Nov)", items: [["dIdeaBoard", "Board (Home and project)", "HomeOverview, ProjectBoard; a card opens the agent view"]] });
  if (location.hash.slice(1) === "dIdeaBoard") S.screen = "dIdeaBoard";
  render();
}
