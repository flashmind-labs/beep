import { projectName } from "./projects-state.ts";
import { MACHINES } from "../machines/machine-store.ts";
import { renameProject } from "./rename.tsx";
import { isMobile, onPhone } from "../ui/platform.ts";
// project/: the Projects page and one project's page (pull requests, issues,
// runs, file sync, custom agents, the agent bar). Entry: this file's view.
// The Projects page: drawing it, and the project rows Home lists.
import { filterProjects, whereRows, forgeLinks, projectLine, REMOVE_PROJECT, type Project } from "./projects-state.ts";
import { overviewGroups, overviewSub } from "./overview-state.ts";
import { openMenu, type MenuItem } from "../ui/menu.tsx";
import { openProjectMenu } from "./syncthing.ts";
import { openProjectSkills } from "./project-skills.tsx";
import { openProjectHarnesses } from "./project-harnesses.tsx";
import { editorsFor } from "./editors.ts";
import { ProjectEditors } from "./header-view.tsx";
import { type ReactNode } from "react";
import { NEW_WINDOW } from "./projectwindow.ts";
import { localProjectId } from "./localid.ts";
import { type CenterView, state, whereOf, projectLoaded, sessionsChanged } from "../center/state.ts";
import { openUrl } from "../ui/tauri.ts";
import { boxOf, currentLocation, drawPage, drawTop, go, windowAlone } from "../center/nav.ts";
import { setProjectOnTop, openProjectWindow, removeProject } from "./projectops.ts";
import { projectRows } from "./projectrows.ts";
import { addProject } from "./addflow.ts";
import { editGroup } from "./groups.tsx";
import { disposeOverview, overviewSection, projectDetail } from "./projects.tsx";
import { NoProjects, Parts, viewPart, type PagePart } from "./page-view.tsx";
import { splitOn } from "./split.tsx";
import { openTile } from "./tiles.tsx";
import { runs, syncProjectFollow } from "./sections.ts";
import { terms } from "./stores.ts";
import { pa, projectChanged } from "./barstate.ts";
import { projectComp, drawProjectComp } from "./composer.tsx";
import { noteRecent, revalidate, revalidateOverview, markDue, prefetchRecentSoon, whereDue, foundSection, elsewhereSection, disposeSessionPanel } from "./loads.tsx";
import { releaseSessions } from "./sessionsload.ts";
import { ensureHome } from "../center/refresh.ts";
import { Icon } from "../ui/icons.tsx";
import { Button, EmptyPanel } from "../ui/kit.tsx";
import { openTeamJoin } from "./teamjoin.tsx";

let focusComposer = false;

/// What the page last drew. It stays rendered while another page shows, so
/// coming back keeps its place.
let shown: { project: string; split: boolean; blank: boolean; parts: PagePart[] } | null = null;

/// A new project gets a new view, so nothing of the last one carries over.
export function ProjectsPage() {
  return shown && <Parts key={shown.project} className={`cc-view${shown.split ? " split" : ""}${shown.blank ? " blank" : ""}`} project={shown.project} parts={shown.parts} />;
}

projectChanged.follow(() => { if (currentLocation()?.view === "projects") drawProjects(); });
projectLoaded.follow(() => {
  if (currentLocation()?.view === "projects") { drawProjects(); drawTop(); }
});
sessionsChanged.follow(project => {
  if (project !== null && currentLocation()?.view === "projects" && state.picked === project) drawProjects();
});

export function drawProjects() {
  const el = boxOf("projects");
  if (!el) return;
  const rows = projectRows();
  if (!state.picked) disposeSessionPanel();
  const parts: PagePart[] = [];
  let one: Project | null = null;
  pa.blank = false;

  if (!state.home) {
    parts.push(viewPart("loading", <EmptyPanel title="Loading projects…" />));
  } else if (!rows || !rows.length) {
    parts.push(viewPart("none", <NoProjects add={addProject} />));
  } else {
    const shownRows = filterProjects(rows, state.picked);
    one = state.picked && shownRows.length === 1 ? shownRows[0] : null;
    parts.push(...(one ? projectDetail(one) : overviewSection(shownRows.length ? shownRows : rows)));

    const found = foundSection(!one);
    if (found) parts.push(viewPart("found", found));
    const elsewhere = elsewhereSection(rows, !state.picked);
    if (elsewhere) parts.push(viewPart("elsewhere", elsewhere));
  }

  syncProjectFollow();

  const project = state.picked ?? "";
  watchScroll(el, project);
  shown = { project, split: !!one && splitOn(one), blank: pa.blank, parts };
  drawPage("projects");
  restoreScroll(el);

  // The bar starts coding agents in the project's checkout. A phone keeps the
  // room for the page and starts them from the + in its top bar.
  if (one && !isMobile()) {
    drawProjectComp(one.name);
    if (el.lastElementChild !== projectComp().form) el.append(projectComp().form);
    if (focusComposer && !el.hidden) {
      focusComposer = false;
      projectComp().input.focus({ preventScroll: true });
    }
  } else if (projectComp().form.parentNode === el) {
    projectComp().form.remove();
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
  title() {
    if (!state.picked) {
      const rows = projectRows();
      const sub = rows?.length ? overviewSub(overviewGroups(rows, state.top, state.groups)) : "Repositories on your machines";
      // Group controls follow mesh support.
      const tools = [state.groups ? <Button key="group" small className="pj-tool" aria-label="New group" onClick={() => editGroup()}><span style={{ display: "contents" }}><Icon name="folder" /></span> New group</Button> : null,
        !isMobile() && !onPhone() ? <Button key="join" small className="pj-tool" aria-label="Join team" onClick={() => openTeamJoin(null)}><span style={{ display: "contents" }}><Icon name="group" /></span> Join team</Button> : null,
        <Button key="add" small className="pj-tool fab" aria-label="Add a project" onClick={addProject}><span style={{ display: "contents" }}><Icon name="plus" /></span> Add a project</Button>];
      return { title: "Projects", sub, reactTools: tools };
    }

    const p = filterProjects(projectRows() || [], state.picked)[0];
    const tools: ReactNode[] = [];

    if (p) {
      // A phone starts agents from here; its page has no bar.
      if (isMobile()) tools.push(<Button key="new" small className="pj-tool fab" aria-label="New agent" onClick={() => go("pane", { draft: { project: p.name, machine: null } })}><span style={{ display: "contents" }}><Icon name="plus" /></span> New agent</Button>);

      // A phone's top bar has no room for them: they go in the menu.
      const forges = forgeLinks(p.url, runs.hosts(localProjectId(p.name)));

      for (const f of isMobile() ? [] : forges) {
        tools.push(<Button key={`forge:${f.url}`} small title={f.url} onClick={() => openUrl(f.url)}>{f.label}</Button>);
      }

      const here = whereRows(p, whereOf(p.name) || null, MACHINES.mac?.id || null).find(r => r.mac && r.folder);
      const editors = editorsFor(here?.folder, () => projectLoaded.notify());
      if (here?.folder && editors.length) tools.push(<ProjectEditors key="editors" editors={editors} folder={here.folder} />);
      // The rest goes in one menu, so the header keeps to a line.
      tools.push(<Button key="more" look="quiet" small title="More" aria-label="More" onClick={(event: React.MouseEvent) => {
        const project = (projectRows() || []).find(row => row.name === p.name) ?? p;
        const on = state.top.includes(project.name);
        const items: MenuItem[] = [{ label: on ? "Unpin" : "Pin", icon: "pin", on: () => setProjectOnTop(project.name, !on) }];
        if (isMobile()) items.push(...forges.map(f => ({ label: `Open on ${f.label}`, icon: "openWindow" as const, on: () => openUrl(f.url) })));
        if (!windowAlone && !onPhone()) items.push({ label: NEW_WINDOW, icon: "openWindow", on: () => openProjectWindow(project.name) });
        if (project.imported) items.push({ label: "Rename", icon: "edit", on: () => renameProject(project) });
        items.push(
          { kind: "separator" },
          { kind: "section", label: "Agents", items: [
            { label: "Skills", icon: "skill", chevron: true, on: () => openProjectSkills(project.name) },
            { label: "Harnesses", icon: "sliders", chevron: true, on: () => openProjectHarnesses(project.name) },
          ] },
        );
        if (project.imported) openProjectMenu(event.nativeEvent, project, items, { label: REMOVE_PROJECT, icon: "remove", warn: true, on: () => removeProject(project.name) }, () => { whereDue.add(project.name); revalidate(project.name); });
        else openMenu(event.nativeEvent, project.name, items);
      }}>⋯</Button>);
    }

    return { title: p ? projectName(p) : state.picked, sub: projectLine(p), reactTools: tools };
  },
  mount(_el, arg) {
    state.picked = arg?.project || null;
    focusComposer = !!state.picked;
    // A build's notification opens its run on the Actions tile.
    const run = arg?.run;
    pa.run = run && state.picked ? { project: state.picked, ...run } : null;

    // What the project last showed draws at once; what changed since its
    // read reads again behind it.
    if (state.picked) {
      markDue(state.picked);
      noteRecent(state.picked);
    }

    openTile(pa.run ? "actions" : arg?.tile ?? null, false);
    drawProjects();

    if (state.picked) revalidate(state.picked);
    else revalidateOverview();

    releaseSessions(state.picked);
    ensureHome();
    prefetchRecentSoon();
  },
  unmount: () => { focusComposer = false; releaseSessions(null); disposeSessionPanel(); disposeOverview(); openTile(null, false); terms.stop(); },
};
