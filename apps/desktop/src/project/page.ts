import { onPhone } from "../ui/platform.ts";
// project/: the Projects page and one project's page (tasks, threads, runs,
// file sync, custom agents, the agent bar). Entry: this file's view.
// The Projects page: drawing it, and the project rows Home lists.
import { filterProjects, whereRows, forgeLinks, projectLine, overviewGroups, overviewSub, REMOVE_PROJECT, type Project } from "./projects-state.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { openProjectMenu } from "./syncthing.ts";
import { editorsFor } from "./editors.ts";
import { ProjectEditors } from "./header-view.tsx";
import { createElement, type ReactNode } from "react";
import { NEW_WINDOW } from "./projectwindow.ts";
import { agentFor, pageAgent } from "./agent.ts";
import { state, warm } from "../center/state.ts";
import { openUrl } from "../center/openitems.ts";
import { registerView, windowProject } from "../center/views.ts";
import { setProjectOnTop, openProjectWindow, removeProject, projectRows, openProjects, addProject } from "../center/sidebar.ts";
import { editGroup } from "./groups.ts";
import { overviewSection, projectDetail } from "./projects.ts";
import { EmptyPanel, NoProjects, Parts, hostPart, viewPart, type PagePart } from "./page-view.tsx";
import { createDocumentScope } from "../ui/document-root.tsx";
import { splitOn, openTile } from "./split.ts";
import { liveMode, pa, redrawProjectsPage, runs, terms } from "./sections.ts";
import { projectComp, drawProjectComp } from "./composer.ts";
import { noteRecent, revalidate, revalidateOverview, markDue, prefetchRecentSoon, whereDue, foundSection, disposeFoundSection, elsewhereSection, disposeElsewhereSection, disposeSessionPanel } from "./loads.ts";
import { ensureHome } from "../center/refresh.ts";
import { IC } from "../ui/icons.tsx";

/// The page's outer view. It stays rendered while another page shows, so
/// coming back keeps its place.
const page = createDocumentScope<"page">();

export function drawProjects(el: HTMLElement) {
  const rows = projectRows();
  if (!state.picked) disposeSessionPanel();
  if (!state.home || !rows?.length) { disposeFoundSection(); disposeElsewhereSection(); }
  const parts: PagePart[] = [];
  let one: Project | null = null;

  if (!state.home) {
    parts.push(viewPart("loading", createElement(EmptyPanel, { text: "Loading projects…", bold: true })));
  } else if (!rows || !rows.length) {
    parts.push(viewPart("none", createElement(NoProjects, { add: addProject })));
  } else {
    const shownRows = filterProjects(rows, state.picked);
    one = state.picked && shownRows.length === 1 ? shownRows[0] : null;
    liveMode.follow(one ? agentFor(state.agents, one.name)?.project_id ?? null : null);
    parts.push(...(one ? projectDetail(one) : overviewSection(shownRows.length ? shownRows : rows)));

    const found = foundSection(!one);
    if (found) parts.push(hostPart(found));
    const elsewhere = elsewhereSection(rows, !state.picked);
    if (elsewhere) parts.push(hostPart(elsewhere));
  }

  // Read before the draw: a node that moves scrolls to 0.
  const old = el.querySelector<HTMLElement>(":scope > .cc-view");
  const top = old?.scrollTop ?? 0;
  const inner = old ? [...old.querySelectorAll<HTMLElement>("[id] .pv-scroll")].flatMap(s => { const id = s.closest("[id]")?.id; return id ? [[id, s.scrollTop] as const] : []; }) : [];
  const cols = [".pj-list", ".pj-det", ".pj-scroll"].map(c => [c, old?.querySelector(c)?.scrollTop ?? 0] as const);

  // A new project gets a new view, so nothing of the last one carries over.
  page.set([{ slot: "page", host: el, identity: state.picked ?? "", content: createElement(Parts, {
    className: one && splitOn(one) ? "cc-view split" : "cc-view", project: state.picked ?? "", parts }) }]);

  const view = el.querySelector<HTMLElement>(":scope > .cc-view");

  if (old && view) {
    if (view.scrollTop !== top) view.scrollTop = top;
    for (const [c, at] of cols) { const col = view.querySelector(c); if (col && col.scrollTop !== at) col.scrollTop = at; }

    for (const [id, at] of inner) {
      const s = view.querySelector<HTMLElement>(`#${CSS.escape(id)} .pv-scroll`);
      if (s && s.scrollTop !== at) s.scrollTop = at;
    }
  }

  // The bar starts coding agents in the project's checkout.
  if (one) {
    drawProjectComp(one.name);
    if (el.lastElementChild !== projectComp.form) el.append(projectComp.form);
  } else {
    projectComp.form.remove();
  }
}

registerView("projects", {
  currentArg: () => ({ project: state.picked, tile: pa.tile }),
  title() {
    if (!state.picked) {
      const rows = projectRows();
      const sub = rows?.length ? overviewSub(overviewGroups(rows, state.overview, state.top, state.groups)) : "Repositories on your machines";
      // Group controls follow mesh support, independently of task summaries.
      const tools = [state.groups ? createElement("button", { key: "group", type: "button", className: "dbtn small pj-tool", "aria-label": "New group", onClick: () => editGroup() }, createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: IC.folder } }), " New group") : null,
        createElement("button", { key: "add", type: "button", className: "dbtn small pj-tool", "aria-label": "Add a project", onClick: addProject }, createElement("span", { style: { display: "contents" }, dangerouslySetInnerHTML: { __html: IC.plus } }), " Add a project")];
      return { title: "Projects", sub, reactTools: tools };
    }

    const p = filterProjects(projectRows() || [], state.picked)[0];
    const tools: ReactNode[] = [];

    if (p) {
      const agent = pageAgent(state.agents, p.name, state.subscribed);

      for (const f of forgeLinks(p.url, agent ? runs.hosts(agent.project_id) : null)) {
        tools.push(createElement("button", { key: `forge:${f.url}`, type: "button", className: "dbtn small", title: f.url, onClick: () => openUrl(f.url) }, f.label));
      }

      const here = whereRows(p, state.where.get(p.name) || null, state.mac?.id || null).find(r => r.mac && r.folder);
      const editors = editorsFor(here?.folder, redrawProjectsPage);
      if (here?.folder && editors.length) tools.push(createElement(ProjectEditors, { key: "editors", editors, folder: here.folder }));
      // The rest goes in one menu, so the header keeps to a line.
      tools.push(createElement("button", { key: "more", type: "button", className: "dbtn quiet small", title: "More", "aria-label": "More", onClick: (event: React.MouseEvent) => {
        const project = (projectRows() || []).find(row => row.name === p.name) ?? p;
        const on = state.top.includes(project.name);
        const items: MenuItem[] = [{ label: on ? "Unpin" : "Pin", on: () => setProjectOnTop(project.name, !on) }];
        if (!windowProject) items.push(...(onPhone() ? [] : [{ label: NEW_WINDOW, on: () => openProjectWindow(project.name) }]), { label: "All projects", on: () => openProjects() });
        if (project.imported) items.push({ label: REMOVE_PROJECT, warn: true, on: () => removeProject(project.name) });
        if (project.imported) openProjectMenu(event.nativeEvent, project, items, () => { whereDue.add(project.name); revalidate(project.name); });
        else openMenu(event.nativeEvent, project.name, items);
      } }, "⋯"));
    }

    return { title: state.picked, sub: projectLine(p), reactTools: tools };
  },
  mount(el, arg) {
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
    drawProjects(el);

    if (state.picked) revalidate(state.picked);
    else revalidateOverview();

    ensureHome();
    prefetchRecentSoon();
  },
  unmount: () => { disposeFoundSection(); disposeElsewhereSection(); disposeSessionPanel(); openTile(null, false); liveMode.follow(null); terms.stop(); },
});
