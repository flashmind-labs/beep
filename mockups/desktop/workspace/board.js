// ---- The board (#613): Home's first tab and a project's front page ----
// Follows center/lanes-view.tsx, center/home-overview.tsx and project/lanes.tsx. Needs you,
// Working, Ready to review and Done today take the place of Home's sections and of the project's
// list and Overview. Settings, Appearance picks Board or List for each page; List is the layout
// the earlier files draw, so the cards here are read from its rows and stay in step with them.
const BOARD_MOCK = (() => {
  document.head.insertAdjacentHTML("beforeend", '<link rel="stylesheet" href="workspace/board.css">');

  const LANES = [["needs", "Needs you"], ["working", "Working"], ["review", "Ready to review"], ["done", "Done today"]];
  const { IC_PR, IC_SYNC, CONFLICTS } = WORKSPACE_MOCK;

  const layout = page => S.d[page === "home" ? "frontHome" : "frontProject"] === "list" ? "list" : "board";
  const mark = agent => `<span class="cc-ag ${agent}">${agent === "codex" ? "CX" : "CC"}</span>`;
  const working = when => `<span class="cc-running"><span class="drun" role="img" aria-label="Working"></span>${when}</span>`;
  const button = (label, attrs, primary = false) => `<button class="dbtn small${primary ? " pri" : ""}" ${attrs}>${label}</button>`;

  // A card: its title, who and where, then what it waits for or since when. Only a card that
  // waits on the user carries a button.
  const card = (c, index) => `<div class="bd-card" role="button" tabindex="0" data-bdcard="${c.key || c.title}" ${c.go || ""} style="${c.dim ? "opacity:.55;" : ""}${S.d.bdlive ? `view-transition-name:bd-card-${index}` : ""}"${c.hint ? ` title="${c.hint}"` : ""}>
      <b>${c.title}</b>
      <span class="sub">${c.lead || ""}<span>${c.sub}</span></span>
      ${c.foot || c.action ? `<span class="ft">${c.foot || ""}<span class="sp"></span>${c.action || ""}</span>` : ""}
    </div>`;
  // `more` makes a lane's title open the full list behind it.
  const lanes = (cards, more = {}) => `<div class="bd-lanes">${LANES.map(([lane, title]) => {
    const own = cards.filter(c => c.lane === lane);
    return `<div class="bd-lane"><h4>${more[lane] ? `<button ${more[lane]}>${title}</button>` : title}<span>${own.length || ""}</span></h4>${own.map(c => card(c, cards.indexOf(c))).join("") || '<div class="bd-empty">Nothing here</div>'}</div>`;
  }).join("")}</div>`;

  const parsed = html => { const t = document.createElement("template"); t.innerHTML = html; return t; };
  const section = (root, title) => [...root.querySelectorAll(".cc-sec")].find(sec => sec.querySelector("h4")?.firstChild?.textContent.trim() === title)?.parentElement ?? null;
  const goOf = row => ["dact", "dgo", "toast"].flatMap(k => row.dataset[k] ? [`data-${k}="${row.dataset[k]}"`] : []).join(" ");
  // What a list row says: its lead, title, muted line and right side.
  const line = row => {
    const text = row.querySelector("button.t"), lead = row.firstElementChild === text ? "" : row.firstElementChild.outerHTML;
    return { lead, title: text.querySelector("b").textContent, sub: text.querySelector(":scope > span")?.innerHTML ?? "", right: row.querySelector(".r"), go: goOf(row), dim: !!row.style.opacity };
  };
  const conflictCards = () => S.d.sy === "insync" ? [] : CONFLICTS.map(([path, note]) => ({ lane: "needs", lead: IC_SYNC, title: path, sub: note, hint: path, go: 'data-dgo="dSync"', action: button("Resolve", 'data-dgo="dSync"') }));

  // Home's board: what waits on the user, then every session once, by its lane.
  function homeCards(view) {
    const needs = section(view, "Needs you"), running = section(view, "Running"), cards = [];
    if (!needs && !running) return null;

    for (const row of needs?.querySelectorAll(".cc-row.cc-line") ?? []) {
      const { right, ...rest } = line(row);
      cards.push({ ...rest, lane: "needs", action: right.innerHTML });
    }

    for (const row of running?.querySelectorAll(".cc-row.cc-line") ?? []) {
      const { right, ...rest } = line(row);
      // A session that asks already has its card in Needs you.
      if (right.querySelector(".cc-tag.wait")) continue;
      const sub = parsed(rest.sub), chip = sub.content.querySelector(".cc-tag");
      chip?.remove();
      const asks = S.d.bdasks && rest.title === ASKS;
      cards.push({ ...rest, sub: sub.innerHTML.trim(), lane: asks ? "needs" : "working", foot: asks ? "" : chip ? chip.outerHTML : rest.dim ? right.innerHTML : working(right.innerHTML), action: asks ? button("Answer", rest.go, true) : "" });
    }
    if (!running || S.d.cc === "old") return cards;

    const open = 'data-dact="dset:ags:done" data-dgo="dAgentSession"';
    cards.push({ lane: "review", lead: mark("codex"), title: "Rewrite the pricing page copy", sub: "you/site · studio", foot: "12m ago", go: open },
      { lane: "done", lead: mark("claude"), title: "Retry uploads when the token expires", sub: "acme/widget · build-box", foot: "2 h ago", go: 'data-dact="dset:ags:ended" data-dgo="dAgentSession"' },
      { lane: "done", lead: mark("codex"), title: "Update the changelog", sub: "acme/api · studio", foot: "5 h ago", go: 'data-dact="dset:ags:ended" data-dgo="dAgentSession"' });
    return cards;
  }
  // The session whose question the lane change plays.
  const ASKS = "Clippy fixes in the sync crate";

  const baseHome = D.dHome;
  D.dHome = (...a) => {
    const html = baseHome(...a);
    if (layout("home") === "list") return html;
    const t = parsed(html), view = t.content.querySelector(".cc-view"), tabs = view?.querySelector(".cc-tabs");
    if (!tabs) return html;

    const first = tabs.querySelector("button");
    if (first.firstChild.textContent === "Overview") first.firstChild.textContent = "Board";
    if (first.getAttribute("aria-selected") !== "true") return t.innerHTML;
    const cards = homeCards(view);
    if (!cards) return t.innerHTML;

    const needs = section(view, "Needs you"), running = section(view, "Running");
    (needs || running).insertAdjacentHTML("beforebegin", `<div class="bd-home" id="sec-needs">${lanes(cards, { working: 'data-dgo="dAgents"', done: 'data-dgo="dAgents"' })}</div>`);
    needs?.remove();
    running?.remove();
    return t.innerHTML;
  };

  // A project's board: its sessions by lane, with the pull requests and sync conflicts that wait on the user.
  function projectCards(overview) {
    const cards = [], project = "acme/widget";
    const rows = title => section(overview, title)?.querySelectorAll(".cc-row.cc-line") ?? [];

    for (const row of rows("Running")) {
      const { right, ...rest } = line(row);
      // A session between turns waits to be read.
      cards.push({ ...rest, lead: mark(/codex/.test(rest.lead) ? "codex" : "claude"), sub: `${project} · ${rest.sub}`, lane: right.querySelector(".dspin") ? "working" : "review", foot: right.innerHTML });
    }

    for (const row of rows("Pull requests")) {
      const { right, lead, ...rest } = line(row), needs = /needs reply/.test(right.textContent);
      if (!needs && !/ready/.test(right.textContent)) continue;
      cards.push({ ...rest, lead: IC_PR, title: `${parsed(lead).content.textContent.trim()} ${rest.title}`, lane: needs ? "needs" : "review", action: button(needs ? "Reply" : "Review", rest.go) });
    }
    cards.push(...conflictCards());

    for (const row of rows("Past sessions")) {
      const { title, sub } = line(row), [agent, machine, ended] = sub.split(" · ");
      // Only the sessions that ended today.
      if (!/^ended \d+ (min|h) ago$/.test(ended)) continue;
      cards.push({ lane: "done", lead: mark(agent === "Codex" ? "codex" : "claude"), title, sub: `${project} · ${machine}`, foot: ended.replace("ended ", ""), go: 'data-dact="dset:ags:ended" data-dgo="dAgentSession"' });
    }

    return cards;
  }

  const baseProject = D.dWinProject;
  D.dWinProject = (...a) => {
    const html = baseProject(...a);
    if (layout("project") === "list") return html;
    const t = parsed(html), view = t.content.querySelector(".cc-view.project-current");
    if (!view) return html;

    const front = view.querySelector('.pj-li.nav[title="Overview"]');
    if (front) { front.title = "Board"; front.querySelector("b").textContent = "Board"; }
    const overview = view.querySelector(".pj-ov");
    if (!overview) return t.innerHTML;

    const pick = id => `data-dact="dset:psel:${id}" data-dgo="${S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject"}"`;
    view.className = "bd-pj project-current";
    view.querySelector(".pj-split").outerHTML = lanes(projectCards(overview), { working: pick("running"), done: pick("agents") });
    return t.innerHTML;
  };

  // The list layout keeps Running in the sidebar; the board has Working instead.
  const baseRender = render;
  render = function () {
    baseRender();
    if (layout("home") === "list") return;
    for (const link of document.querySelectorAll("#desk .cc-side .cc-si")) if (link.querySelector(".tt")?.textContent === "Running") link.remove();
  };

  // A session's card grows into the session's head as it opens.
  const still = () => matchMedia("(prefers-reduced-motion: reduce)").matches || !document.startViewTransition;
  document.addEventListener("click", event => {
    const opened = event.target.closest(".bd-card");
    if (!opened || event.target.closest(".bd-card button") || opened.dataset.dgo !== "dAgentSession" || still()) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    opened.style.viewTransitionName = "bd-hero";
    document.documentElement.dataset.bdmove = "open";
    const [, key, value] = (opened.dataset.dact || "").split(":");
    const done = () => { delete document.documentElement.dataset.bdmove; };
    document.startViewTransition(() => { if (key) S.d[key] = value; go("dAgentSession"); }).finished.then(done, done);
  }, { capture: true });

  // Lane changes come from agents, not from a click: the app batches them and skips them when hidden.
  function laneChange() {
    Object.assign(S.d, { frontHome: "", htab: "overview", cc: "default", mrm: "", tm: "" });
    const change = () => { S.d.bdasks = !S.d.bdasks; render(); };
    if (still()) return change();
    S.d.bdlive = true;
    render();
    document.documentElement.dataset.bdmove = "lane";
    const done = () => { S.d.bdlive = false; delete document.documentElement.dataset.bdmove; render(); };
    document.startViewTransition(change).finished.then(done, done);
  }

  const plain = () => (S.d.htab || "overview") === "overview" && (S.d.cc || "default") === "default";
  const tab = VARIANTS.dHome.find(([label]) => label === "Tab: Overview");
  if (tab) tab[0] = "Tab: Board";
  const at = VARIANTS.dHome.findIndex(([label]) => label === "Tab: Machines") + 1;
  VARIANTS.dHome.splice(at, 0,
    ["Layout: List", () => { Object.assign(S.d, { frontHome: layout("home") === "list" ? "" : "list", htab: "overview", cc: "default" }); render(); }, () => layout("home") === "list"],
    ["Play: a card changes lane", laneChange, () => !!S.d.bdasks && plain() && layout("home") === "board"]);
  const front = VARIANTS.dWinProject.find(([label]) => label === "Overview");
  if (front) front[0] = "Board";
  VARIANTS.dWinProject.splice(VARIANTS.dWinProject.indexOf(front) + 1, 0,
    ["Layout: List", () => { S.d.frontProject = layout("project") === "list" ? "" : "list"; S.d.psel = "overview"; S.d.pmenu = ""; render(); }, () => layout("project") === "list"]);

  // A two-finger swipe sideways goes back or forward through the screens visited. The page
  // follows the fingers, leaves past 120px and comes back when they stop short. What scrolls
  // sideways under the pointer keeps the swipe, and one swipe is one step.
  const visited = [], SWIPE_PX = 120, AIM_PX = 12, REST_MS = 150;
  let here = -1, stepping = false;
  const baseGo = go;
  go = function (screen) {
    if (here < 0) { visited.push(S.screen); here = 0; }
    if (!stepping && screen !== S.screen) { visited.splice(here + 1, visited.length, screen); here = visited.length - 1; }
    baseGo(screen);
  };
  const page = () => document.querySelector("#desk .cc-main") || document.getElementById("desk");
  const scrollsSideways = (target, forward) => {
    for (let el = target; el instanceof Element; el = el.parentElement) {
      const room = el.scrollWidth - el.clientWidth;
      if (room > 1 && /auto|scroll/.test(getComputedStyle(el).overflowX) && (forward ? el.scrollLeft < room - 1 : el.scrollLeft > 0)) return true;
    }
    return false;
  };
  let phase = "idle", x = 0, y = 0, last = 0, dx = 0, forward = false, rest;
  // The moved page is cut where it would cross the sidebar, whose background lets it show through.
  const shifted = by => ({ transform: `translateX(${by}px)`, clipPath: `inset(0 0 0 ${Math.max(0, -by)}px)` });
  const settle = () => {
    phase = "idle";
    const el = page();
    if (!dx) return;
    el.animate([shifted(dx), shifted(0)], { duration: 180, easing: "cubic-bezier(.2, .8, .2, 1)" });
    dx = 0;
    Object.assign(el.style, { transform: "", clipPath: "" });
  };
  const leave = () => {
    phase = "spent";
    const side = forward ? -1 : 1;
    const step = () => {
      here += forward ? 1 : -1;
      stepping = true;
      go(visited[here]);
      stepping = false;
      if (!still()) page().animate([{ opacity: .3, ...shifted(-56 * side) }, { opacity: 1, ...shifted(0) }], { duration: 200, easing: "cubic-bezier(.2, .8, .2, 1)" });
    };
    if (still()) return step();
    const el = page();
    const out = el.animate([{ ...shifted(dx), opacity: 1 }, { ...shifted((SWIPE_PX + 60) * side), opacity: 0 }], { duration: 90, easing: "ease-in", fill: "forwards" });
    Object.assign(el.style, { transform: "", clipPath: "" });
    dx = 0;
    const land = () => { step(); out.cancel(); };
    out.finished.then(land, land);
  };
  document.addEventListener("wheel", event => {
    if (phase !== "follow" && event.timeStamp - last > REST_MS) phase = "idle";
    last = event.timeStamp;
    if (phase === "spent") return;
    if (phase === "idle") {
      x = 0; y = 0;
      phase = event.target.closest("#desk") && !event.ctrlKey && !event.shiftKey ? "aim" : "spent";
      if (phase === "spent") return;
    }
    x += event.deltaX; y += event.deltaY;

    if (phase === "aim") {
      if (Math.hypot(x, y) < AIM_PX) return;
      forward = x > 0;
      const has = forward ? here >= 0 && here < visited.length - 1 : here > 0;
      if (Math.abs(x) < 2 * Math.abs(y) || !has || scrollsSideways(event.target, forward)) { phase = "spent"; return; }
      phase = "follow";
    }

    const travel = Math.max(0, forward ? x : -x);
    clearTimeout(rest);
    if (travel >= SWIPE_PX) return leave();
    rest = setTimeout(settle, REST_MS);
    if (!still()) Object.assign(page().style, shifted(dx = forward ? -travel : travel));
  }, { passive: true });

  return { layout, lanes, card, mark, button };
})();
