import { onPhone } from "../ui/platform.ts";
// project/: the Projects page and one project's page (tasks, threads, runs,
// file sync, custom agents, the agent bar). Entry: this file's view.
// The Projects page: drawing it, and the project rows Home lists.
import { dbtn as button, emptyPanel, svgEl as icon } from "../ui/parts.ts";
import { filterProjects, whereRows, forgeLinks, projectLine, overviewGroups, overviewSub, REMOVE_PROJECT, type Project } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { openMenu, type MenuItem } from "../ui/menu.ts";
import { openProjectMenu } from "./syncthing.ts";
import { editorTools } from "./editors.ts";
import { NEW_WINDOW } from "./projectwindow.ts";
import { agentFor, pageAgent } from "./agent.ts";
import { state, pages, warm } from "../center/state.ts";
import { openUrl } from "../center/openitems.ts";
import { registerView, windowProject } from "../center/views.ts";
import { setProjectOnTop, openProjectWindow, removeProject, projectRows, openProjects, loadFound } from "../center/sidebar.ts";
import { editGroup } from "./groups.ts";
import { importButton, overviewSection, loadOverview, projectDetail } from "./projects.ts";
import { splitOn, openTile, projectPanel } from "./split.ts";
import { liveMode, pa, redrawProjectsPage, runs, terms } from "./sections.ts";
import { projectComp, drawProjectComp } from "./composer.ts";
import { noteRecent, revalidate, prefetchRecentSoon, whereDue, gitDue, sessionsDue, foundSection, elsewhereSection, sessionPanel, disposeSessionPanel } from "./loads.ts";
import { refresh } from "../center/refresh.ts";
import { IC } from "../ui/icons.ts";
import { resetRows } from "../ui/lazyrows.ts";
import { patchChildren } from "../ui/patch.ts";

// These wrappers have no event handlers. Keep them attached while their
// independent sections change, so a sync update cannot interrupt a click.
const layouts = ".pj-split, .pj-list, .pj-det, .pj-ov, .pj-ov > div, .pj-cols, .ca-col, #sec-sessions";

function patchProjectView(current: Element, next: Element) {
  const replacedPanels = new Set<number>();
  const children = [...next.children].map((child, i) => {
    const old = current.children[i];
    const panel = projectPanel(child) ?? sessionPanel(child);
    if (panel) { if (old !== panel) replacedPanels.add(i); return panel; }
    const split = old?.classList.contains("pj-split") && child.classList.contains("pj-split");
    if (!old || old.tagName !== child.tagName || (!split && old.className !== child.className) || !old.matches(layouts)) {
      if (child.matches(layouts)) patchProjectView(child, child);
      return child;
    }
    if (split && old.className !== child.className) old.className = child.className;
    patchProjectView(old, child);
    return old;
  });
  patchChildren(current, children, i => replacedPanels.has(i));
}

export function drawProjects(el: HTMLElement) {
  const rows = projectRows();
  if (!state.picked) disposeSessionPanel();
  let view: HTMLElement = h("div", "cc-view");
  view.dataset.project = state.picked ?? "";
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

    if (one) {
      view.append(...projectDetail(one));
      if (splitOn(one)) view.classList.add("split");
    } else view.append(...overviewSection(shownRows.length ? shownRows : rows));

    const found = one ? null : foundSection();
    if (found) view.append(found);
    const elsewhere = !state.picked ? elsewhereSection(rows) : null;
    if (elsewhere) view.append(elsewhere);
  }

  // Patch within one project; changing projects installs new event handlers
  // even when the two projects happen to have the same markup.
  const old = el.querySelector<HTMLElement>(":scope > .cc-view");

  if (old) {
    // Read before the swap: a detached element scrolls to 0.
    const top = old.scrollTop;
    const inner = new Map([...old.querySelectorAll<HTMLElement>("[id] .pv-scroll")].map(s => [s.closest("[id]")!.id, s.scrollTop]));
    const cols = [".pj-list", ".pj-det", ".pj-scroll"].map(c => [c, old.querySelector(c)?.scrollTop ?? 0] as const);

    if (old.dataset.project === view.dataset.project && old.className === view.className) {
      patchProjectView(old, view);
      view = old;
    } else {
      patchProjectView(view, view);
      old.replaceWith(view);
    }

    if (view.scrollTop !== top) view.scrollTop = top;
    for (const [c, at] of cols) { const col = view.querySelector(c); if (col && col.scrollTop !== at) col.scrollTop = at; }

    for (const [id, at] of inner) {
      const s = view.querySelector<HTMLElement>(`#${CSS.escape(id)} .pv-scroll`);
      if (s && s.scrollTop !== at) s.scrollTop = at;
    }
  } else {
    patchProjectView(view, view);
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
  currentArg: () => ({ project: state.picked, tile: pa.tile }),
  title() {
    if (!state.picked) {
      const rows = projectRows();
      const sub = rows?.length ? overviewSub(overviewGroups(rows, state.overview, state.top, state.groups)) : "Repositories on your machines";
      // Group controls follow mesh support, independently of task summaries.
      const tools = state.groups ? [button("", "small", () => editGroup())] : [];
      if (tools.length) tools[0].append(icon(IC.folder)!, " New group");
      return { title: "Projects", sub, tools: [...tools, importButton("small")] };
    }

    const p = filterProjects(projectRows() || [], state.picked)[0];
    const tools = [];

    if (p) {
      const agent = pageAgent(state.agents, p.name, state.subscribed);

      for (const f of forgeLinks(p.url, agent ? runs.hosts(agent.project_id) : null)) {
        const b = button(f.label, "small", () => openUrl(f.url));
        b.title = f.url;
        tools.push(b);
      }

      const here = whereRows(p, state.where.get(p.name) || null, state.mac?.id || null).find(r => r.mac && r.folder);
      tools.push(...editorTools(here?.folder, redrawProjectsPage));
      // The rest goes in one menu, so the header keeps to a line.
      const more = button("⋯", "quiet small", e => {
        const on = state.top.includes(p.name);
        const items: MenuItem[] = [{ label: on ? "Unpin" : "Pin", on: () => setProjectOnTop(p.name, !on) }];
        if (!windowProject) items.push(...(onPhone() ? [] : [{ label: NEW_WINDOW, on: () => openProjectWindow(p.name) }]), { label: "All projects", on: () => openProjects() });
        if (p.imported) items.push({ label: REMOVE_PROJECT, warn: true, on: () => removeProject(p.name) });
        if (p.imported) openProjectMenu(e, p, items, () => { whereDue.add(p.name); revalidate(p.name); });
        else openMenu(e, p.name, items);
      });
      more.title = "More";
      more.setAttribute("aria-label", "More");
      tools.push(more);
    }

    return { title: state.picked, sub: projectLine(p), tools };
  },
  mount(el, arg) {
    state.picked = arg?.project || null;
    // A build's notification opens its run on the Actions tile.
    const run = arg?.run;
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

    openTile(pa.run ? "actions" : arg?.tile ?? null, false);
    resetRows("projects-overview");
    drawProjects(el);

    if (state.picked) revalidate(state.picked);
    else { loadFound(); loadOverview(); }

    refresh();
    prefetchRecentSoon();
  },
  unmount: () => { disposeSessionPanel(); openTile(null, false); liveMode.follow(null); terms.stop(); },
});
