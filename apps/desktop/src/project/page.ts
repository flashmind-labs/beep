import { onPhone } from "../ui/platform.ts";
// project/: the Projects page and one project's page (pull requests, issues,
// runs, file sync, custom agents, the agent bar). Entry: this file's view.
// The Projects page: drawing it, and the project rows Home lists.
import { filterProjects, whereRows, forgeLinks, projectLine, REMOVE_PROJECT, type Project } from "./projects-state.ts";
import { overviewGroups, overviewSub } from "./overview-state.ts";
import { openMenu, type MenuAction } from "../ui/menu.ts";
import { openProjectMenu } from "./syncthing.ts";
import { editorsFor } from "./editors.ts";
import { ProjectEditors } from "./header-view.tsx";
import { createElement, type ReactNode } from "react";
import { NEW_WINDOW } from "./projectwindow.ts";
import { localProjectId } from "./localid.ts";
import { type CenterView, state, warm } from "../center/state.ts";
import { openUrl } from "../center/openitems.ts";
import { boxOf, drawPage, windowProject } from "../center/views.ts";
import { setProjectOnTop, openProjectWindow, removeProject, projectRows, addProject } from "../center/sidebar.ts";
import { editGroup } from "./groups.ts";
import { disposeOverview, overviewSection, projectDetail } from "./projects.ts";
import { NoProjects, Parts, viewPart, type PagePart } from "./page-view.tsx";
import { splitOn, openTile } from "./split.ts";
import { liveMode, pa, redrawProjectsPage, runs, terms } from "./sections.ts";
import { projectComp, drawProjectComp } from "./composer.ts";
import { noteRecent, revalidate, revalidateOverview, markDue, prefetchRecentSoon, whereDue, foundSection, elsewhereSection, disposeSessionPanel } from "./loads.ts";
import { ensureHome } from "../center/refresh.ts";
import { IC } from "../ui/icons.tsx";
import { EmptyPanel } from "../ui/kit.tsx";

/// What the page last drew. It stays rendered while another page shows, so
/// coming back keeps its place.
let shown: { project: string; split: boolean; parts: PagePart[] } | null = null;

/// A new project gets a new view, so nothing of the last one carries over.
export function ProjectsPage() {
  return shown && createElement(Parts, { key: shown.project, className: shown.split ? "cc-view split" : "cc-view", project: shown.project, parts: shown.parts });
}

export function drawProjects() {
  const el = boxOf("projects");
  if (!el) return;
  const rows = projectRows();
  if (!state.picked) disposeSessionPanel();
  const parts: PagePart[] = [];
  let one: Project | null = null;

  if (!state.home) {
    parts.push(viewPart("loading", createElement(EmptyPanel, { title: "Loading projects…" })));
  } else if (!rows || !rows.length) {
    parts.push(viewPart("none", createElement(NoProjects, { add: addProject })));
  } else {
    const shownRows = filterProjects(rows, state.picked);
    one = state.picked && shownRows.length === 1 ? shownRows[0] : null;
    liveMode.follow(one ? localProjectId(one.name) : null);
    parts.push(...(one ? projectDetail(one) : overviewSection(shownRows.length ? shownRows : rows)));

    const found = foundSection(!one);
    if (found) parts.push(viewPart("found", found));
    const elsewhere = elsewhereSection(rows, !state.picked);
    if (elsewhere) parts.push(viewPart("elsewhere", elsewhere));
  }

  const project = state.picked ?? "";
  watchScroll(el, project);
  shown = { project, split: !!one && splitOn(one), parts };
  drawPage("projects");
  restoreScroll(el);

  // The bar starts coding agents in the project's checkout.
  if (one) {
    drawProjectComp(one.name);
    if (el.lastElementChild !== projectComp.form) el.append(projectComp.form);
  } else {
    projectComp.form.remove();
  }
}

/// Where the page's scrollers stand, by selector within the page view ("" for
/// the view itself), kept as they scroll rather than read around a draw,
/// which would lay the page out on every open. A node the draw moves scrolls
/// to 0, so one at 0 is put back; one that scrolled since, as a list keeping
/// its rows in place does, stays. Another project starts with none.
const scrolled = new Map<string, number>();
let scrolledProject: string | null = null;
const watchedBoxes = new WeakSet<HTMLElement>();
const COLUMNS = [".pj-list", ".pj-det", ".pj-scroll"];

function watchScroll(el: HTMLElement, project: string) {
  if (scrolledProject !== project) {
    scrolled.clear();
    scrolledProject = project;
  }

  if (watchedBoxes.has(el)) return;
  watchedBoxes.add(el);
  el.addEventListener("scroll", event => {
    const box = event.target, view = el.querySelector<HTMLElement>(":scope > .cc-view");
    if (!(box instanceof HTMLElement) || !view) return;
    if (box === view) scrolled.set("", box.scrollTop);
    for (const column of COLUMNS) if (box.matches(column) && view.querySelector(column) === box) scrolled.set(column, box.scrollTop);
    const id = box.matches(".pv-scroll") ? box.closest("[id]")?.id : null;
    if (id) scrolled.set(`#${CSS.escape(id)} .pv-scroll`, box.scrollTop);
  }, { capture: true, passive: true });
}

function restoreScroll(el: HTMLElement) {
  const view = el.querySelector<HTMLElement>(":scope > .cc-view");
  if (!view) return;

  for (const [selector, at] of scrolled) {
    const box = selector ? view.querySelector(selector) : view;
    if (at && box && box.scrollTop === 0) box.scrollTop = at;
  }
}

export const projectsView: CenterView<"projects"> = {
  currentArg: () => ({ project: state.picked, tile: pa.tile }),
  title() {
    if (!state.picked) {
      const rows = projectRows();
      const sub = rows?.length ? overviewSub(overviewGroups(rows, state.top, state.groups)) : "Repositories on your machines";
      // Group controls follow mesh support.
      const tools = [state.groups ? createElement("button", { key: "group", type: "button", className: "dbtn small pj-tool", "aria-label": "New group", onClick: () => editGroup() }, createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: IC.folder } }), " New group") : null,
        createElement("button", { key: "add", type: "button", className: "dbtn small pj-tool", "aria-label": "Add a project", onClick: addProject }, createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: IC.plus } }), " Add a project")];
      return { title: "Projects", sub, reactTools: tools };
    }

    const p = filterProjects(projectRows() || [], state.picked)[0];
    const tools: ReactNode[] = [];

    if (p) {
      for (const f of forgeLinks(p.url, runs.hosts(localProjectId(p.name)))) {
        tools.push(createElement("button", { key: `forge:${f.url}`, type: "button", className: "dbtn small", title: f.url, onClick: () => openUrl(f.url) }, f.label));
      }

      const here = whereRows(p, state.where.get(p.name) || null, state.mac?.id || null).find(r => r.mac && r.folder);
      const editors = editorsFor(here?.folder, redrawProjectsPage);
      if (here?.folder && editors.length) tools.push(createElement(ProjectEditors, { key: "editors", editors, folder: here.folder }));
      // The rest goes in one menu, so the header keeps to a line.
      tools.push(createElement("button", { key: "more", type: "button", className: "dbtn quiet small", title: "More", "aria-label": "More", onClick: (event: React.MouseEvent) => {
        const project = (projectRows() || []).find(row => row.name === p.name) ?? p;
        const on = state.top.includes(project.name);
        const items: MenuAction[] = [{ label: on ? "Unpin" : "Pin", on: () => setProjectOnTop(project.name, !on) }];
        if (!windowProject && !onPhone()) items.push({ label: NEW_WINDOW, on: () => openProjectWindow(project.name) });
        if (project.imported) openProjectMenu(event.nativeEvent, project, items, { label: REMOVE_PROJECT, warn: true, on: () => removeProject(project.name) }, () => { whereDue.add(project.name); revalidate(project.name); });
        else openMenu(event.nativeEvent, project.name, items);
      } }, "⋯"));
    }

    return { title: state.picked, sub: projectLine(p), reactTools: tools };
  },
  mount(_el, arg) {
    state.picked = arg?.project || null;
    // A build's notification opens its run on the Actions tile.
    const run = arg?.run;
    pa.run = run && state.picked ? { project: state.picked, ...run } : null;

    // What the project last showed draws at once; what changed since its
    // read reads again behind it.
    if (state.picked) {
      warm(state.picked);
      markDue(state.picked);
      noteRecent(state.picked);
    }

    openTile(pa.run ? "actions" : arg?.tile ?? null, false);
    drawProjects();

    if (state.picked) revalidate(state.picked);
    else revalidateOverview();

    ensureHome();
    prefetchRecentSoon();
  },
  unmount: () => { disposeSessionPanel(); disposeOverview(); openTile(null, false); liveMode.follow(null); terms.stop(); },
};
