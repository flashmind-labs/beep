// project/: the Projects page and one project's page (tasks, threads, runs,
// file sync, custom agents, the agent bar). Entry: this file's view.
// The Projects page: drawing it, and the project rows Home lists.
import { ago, plural } from "../ui/format.ts";
import { dbtn as button, emptyPanel, svgEl as icon } from "../ui/parts.ts";
import { filterProjects, whereRows, openOnLabel, projectLine, overviewGroups, overviewSub, REMOVE_PROJECT, type Project } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { editorTools } from "./editors.ts";
import { NEW_WINDOW } from "./projectwindow.ts";
import { agentFor } from "./agent.ts";
import { type ProjectsArg, state, pages, warm } from "../center/state.ts";
import { projectDot, section, line } from "../ui/pieces.ts";
import { openUrl } from "../center/openitems.ts";
import { registerView, windowProject } from "../center/views.ts";
import { setProjectOnTop, openProjectWindow, newWindowOn, removeProject, projectRows, openProjects, loadFound } from "../center/sidebar.ts";
import { editGroup } from "./groups.ts";
import { importButton, projectTable, overviewSection, loadOverview, projectDetail } from "./projects.ts";
import { splitOn, openTile } from "./split.ts";
import { liveMode, pa, redrawProjectsPage } from "./sections.ts";
import { projectComp, drawProjectComp } from "./composer.ts";
import { noteRecent, revalidate, prefetchRecentSoon, whereDue, gitDue, sessionsDue, foundSection, elsewhereSection } from "./loads.ts";
import { refresh } from "../center/refresh.ts";
import { IC } from "../ui/icons.ts";
import { lazyRows, resetRows } from "../ui/lazyrows.ts";

export function drawProjects(el: HTMLElement) {
  const rows = projectRows();
  const view = h("div", "cc-view");
  let one: Project | null = null;
  if (!state.home) {
    view.append(emptyPanel(h("b", "", "Loading projects…")));
  } else if (!rows || !rows.length) {
    view.append(h("div", "cc-panel cc-empty",
      h("b", "", "No projects yet"),
      h("span", "", "A folder or a repository to clone."),
      importButton("pri")));
  } else {
    const shownRows = filterProjects(rows, state.picked);
    one = state.picked && shownRows.length === 1 ? shownRows[0] : null;
    liveMode.follow(one ? agentFor(state.agents, one.name)?.project_id ?? null : null);
    if (one) view.append(...projectDetail(one));
    if (one && splitOn(one)) view.classList.add("split");
    else if (!state.picked && state.overview) view.append(...overviewSection(rows));
    else view.append(section("sec-projects", "Projects", state.picked ? "" : rows.length, null, projectTable(shownRows.length ? shownRows : rows)));
    const found = one ? null : foundSection();
    if (found) view.append(found);
    const elsewhere = !state.picked && state.overview ? elsewhereSection(rows) : null;
    if (elsewhere) view.append(elsewhere);
  }
  // Only the view is drawn again: the composer keeps what is typed and its
  // focus, and the view keeps its scroll.
  const old = el.querySelector(":scope > .cc-view");
  if (old) {
    // Read before the swap: a detached element scrolls to 0.
    const top = old.scrollTop;
    const inner = new Map([...old.querySelectorAll<HTMLElement>("[id] .pv-scroll")].map(s => [s.closest("[id]")!.id, s.scrollTop]));
    const cols = [".pj-list", ".pj-det", ".pj-scroll"].map(c => [c, old.querySelector(c)?.scrollTop ?? 0] as const);
    old.replaceWith(view);
    view.scrollTop = top;
    for (const [c, at] of cols) { const col = view.querySelector(c); if (col) col.scrollTop = at; }
    for (const [id, at] of inner) {
      const s = view.querySelector<HTMLElement>(`#${CSS.escape(id)} .pv-scroll`);
      if (s) s.scrollTop = at;
    }
  } else {
    el.prepend(view);
  }
  // The bar starts coding agents in the project's checkout.
  if (one) {
    drawProjectComp(one.name);
    if (projectComp.form.parentNode !== el) el.append(projectComp.form);
  } else {
    projectComp.form.remove();
  }
}

registerView("projects", {
  title() {
    if (!state.picked) {
      const rows = projectRows();
      const sub = state.overview && rows?.length ? overviewSub(overviewGroups(rows, state.overview, state.top, state.groups)) : "Repositories on your machines";
      // Groups show on the overview, from a server that keeps them.
      const tools = state.overview && state.groups ? [button("", "small", () => editGroup())] : [];
      if (tools.length) tools[0].append(icon(IC.folder)!, " New group");
      return { title: "Projects", sub, tools: [...tools, importButton("small")] };
    }
    const p = filterProjects(projectRows() || [], state.picked)[0];
    const tools = [];
    if (p?.url) {
      const url = p.url, label = openOnLabel(url);
      if (label) tools.push(button(label.replace(/^Open on /, ""), "small", () => openUrl(url)));
    }
    if (p) {
      const here = whereRows(p, state.where.get(p.name) || null, state.mac?.id || null).find(r => r.mac && r.folder);
      tools.push(...editorTools(here?.folder, redrawProjectsPage));
      // The rest goes in one menu, so the header keeps to a line.
      const more = button("⋯", "quiet small", e => {
        const on = state.top.includes(p.name);
        const items: MenuItem[] = [{ label: on ? "Unpin" : "Pin", on: () => setProjectOnTop(p.name, !on) }];
        if (!windowProject) items.push({ label: NEW_WINDOW, on: () => openProjectWindow(p.name) }, { label: "All projects", on: () => openProjects() });
        if (p.imported) items.push({ label: REMOVE_PROJECT, warn: true, on: () => removeProject(p.name) });
        openMenu(e, p.name, items);
      });
      more.title = "More";
      more.setAttribute("aria-label", "More");
      tools.push(more);
    }
    return { title: state.picked, sub: projectLine(p, state.home?.projects, Date.now()), tools };
  },
  mount(el, arg) {
    state.picked = (arg as ProjectsArg | undefined)?.project || null;
    // A build's notification opens its run on the Actions tile.
    const run = (arg as ProjectsArg | undefined)?.run;
    pa.run = run && state.picked ? { project: state.picked, ...run } : null;
    // What the project last showed draws at once; what is due reads again
    // behind it.
    if (state.picked) {
      warm(state.picked);
      if (pages.due("sessions", state.picked)) sessionsDue.add(state.picked);
      if (pages.due("where", state.picked)) whereDue.add(state.picked);
      if (pages.due("gitsync", state.picked)) gitDue.add(state.picked);
      noteRecent(state.picked);
    }
    openTile(pa.run ? "actions" : null, false);
    resetRows("projects-overview");
    drawProjects(el);
    if (state.picked) revalidate(state.picked);
    else { loadFound(); loadOverview(); }
    refresh();
    prefetchRecentSoon();
  },
  unmount: () => openTile(null, false),
});

/// A project's row on Home: its dot, `name` (its full name by default),
/// the machines it is on and how many agents run there.
export function homeProjectRow(p: Project, i: number, name = p.name) {
  const row = line(
    projectDot(i),
    name,
    [p.machines.map(m => m.name).join(", "), p.agents ? plural(p.agents, "agent", "agents") : ""].filter(Boolean).join(" · "),
    [p.last ? ago(p.last) : ""],
    () => openProjects(p.name));
  newWindowOn(row, row, p.name);
  return row;
}

/// Home's Projects: a panel per owner, as many across as fit, so one owner
/// keeps one column's width.
export function projectsSection(rows: Project[]) {
  const owners = new Map<string, { p: Project; i: number }[]>();
  rows.forEach((p, i) => {
    const cut = p.name.lastIndexOf("/");
    const owner = cut > 0 ? p.name.slice(0, cut) : "";
    const list = owners.get(owner);
    if (list) list.push({ p, i }); else owners.set(owner, [{ p, i }]);
  });
  const all = [...owners];
  // A few panels at a time; the rest draw as the page scrolls toward them.
  const panels = lazyRows("home:projects", "", all.length, (from, to) => all.slice(from, to).map(([owner, list]) =>
    section("", owner || "Other", list.length, null, h("div", "cc-panel", ...list.map(({ p, i }) => homeProjectRow(p, i, p.name.slice(p.name.lastIndexOf("/") + 1)))))), 0, 2);
  const el = h("div", "cc-hgrid", ...panels);
  el.id = "sec-projects-home";
  return el;
}
