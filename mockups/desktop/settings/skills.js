// Skill libraries (#674): named packs of agent skills kept out of the agents' global folders.
// Settings > Skills lists them, adds one from Git or a folder, offers the ones found on the
// machines, and installs or uninstalls a library globally. A project's menu has Skills…, a
// checklist that installs libraries, or single skills, into that project on every machine.
const SETTINGS_SKILLS = (() => {
  const { sm, dsetGo } = SETTINGS_ACCOUNTS;
  const { chip, gitRow, note, cd } = SETTINGS_CREDENTIALS;
  const BOOK = `<svg viewBox="0 0 24 24" width="15" height="15" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5z"/><path d="M4 19.5A2.5 2.5 0 0 0 6.5 22H20v-5"/></svg>`;
  const LIBS = [
    { name: "firecrawl", source: "git.example.com/tools/firecrawl-skills", ver: "a3f91c2", global: false, projects: ["acme/widget", "acme/docs"],
      skills: [["firecrawl-scrape", "Scrape one page to Markdown"], ["firecrawl-crawl", "Crawl a site and collect its pages"], ["firecrawl-map", "List a site's URLs"], ["firecrawl-extract", "Pull structured data with a schema"]],
      // Where each skill is installed, for the open list: "global", project names, or nothing.
      at: { "firecrawl-scrape": "global", "firecrawl-crawl": ["acme/widget", "acme/docs"], "firecrawl-map": ["acme/widget"] } },
    { name: "3d", source: "~/libraries/3d", ver: "folder", global: false, projects: ["acme/game"],
      skills: [["blender-modeling", "Model meshes with Blender's Python API"], ["blender-materials", "Build node materials"], ["blender-render", "Render stills and turntables"], ["blender-rigging", "Rig and weight characters"], ["gltf-export", "Export and check glTF files"]] },
    { name: "writing", source: "git.example.com/team/writing-skills", ver: "77de0b4", global: true, projects: [],
      skills: [["release-notes", "Write release notes from commits"], ["plain-style", "Edit text to the house style"]] },
  ];
  const FOUND = [
    { name: "ui-kit", source: "git.example.com/acme/ui-kit-skills", where: "~/.claude/skills on build-box", skills: 3 },
    { name: "data-tools", source: "git.example.com/acme/data-tools", where: "~/.agents/skills on mac-box", skills: 2 },
  ];
  const head = (t, top) => `<div style="font-size:11px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;color:var(--d-dim);margin:${top}px 0 2px">${t}</div>`;
  const where = l => l.global ? chip("Global on every machine", "In each agent's skills folder") : l.projects.length ? chip(`In ${l.projects.length} project${l.projects.length === 1 ? "" : "s"}`, l.projects.join(", ")) : chip("Not installed");
  const libMenu = l => `<div class="dmenu" role="menu" aria-label="${l.name}" style="left:auto;right:0;top:calc(100% + 4px);width:220px;z-index:9">
      <button class="mi" role="menuitem" ${dsetGo("skills")} data-toast="${l.global ? `${l.name} removed from every machine's skills folder` : `${l.name} installed on every machine`}"><b>${l.global ? "Uninstall globally" : "Install globally"}</b></button>
${l.projects.length ? `
      <button class="mi" role="menuitem" ${dsetGo("skills")} data-toast="${l.name} uninstalled from ${l.projects.join(", ")}"><b>Uninstall from ${l.projects.length} project${l.projects.length === 1 ? "" : "s"}</b></button>` : ""}
      <button class="mi" role="menuitem" ${dsetGo("skills")} data-toast="${l.name} updated on every machine"><b>Update</b></button>
      <div role="separator" style="height:1px;margin:4px 3px;background:var(--d-border)"></div>
      <button class="mi" role="menuitem" ${dsetGo("skills")} data-toast="${l.name} removed. Its installed copies are uninstalled"><b style="color:var(--d-err)">Remove library…</b></button></div>`;
  const libRow = (l, v) => {
    const open = v === "skills-open" && l.name === "firecrawl", menu = v === "skills-menu" && l.name === "firecrawl";
    const acts = `<button class="dbtn quiet" ${sm} ${dsetGo(open ? "skills" : "skills-open")} aria-expanded="${open}">${open ? "Hide skills" : "Skills"}</button><span style="position:relative"><button class="dbtn quiet" ${sm} aria-label="More for ${l.name}" aria-haspopup="menu" aria-expanded="${menu}" ${dsetGo(menu ? "skills" : "skills-menu")}>&#8943;</button>${menu ? libMenu(l) : ""}</span>`;
    // Each skill says where it is installed; Uninstall takes it out of every project and the global folders.
    const skillLine = ([n, d]) => {
      const at = l.at?.[n] ?? (l.global ? "global" : []);
      const place = at === "global" ? "Global" : at.length ? at.join(", ") : "Not installed";
      const act = at === "global" || at.length
        ? `<button class="dbtn quiet" ${sm} ${dsetGo("skills-open")} data-toast="${n} uninstalled from ${at === "global" ? "every machine's skills folder" : at.join(", ")}">Uninstall</button>`
        : `<button class="dbtn quiet" ${sm} ${dsetGo("skills-open")} data-toast="${n} installed on every machine">Install globally</button>`;
      return `<div style="display:flex;gap:10px;align-items:center;font-size:12.5px"><code style="font-size:11.5px;min-width:150px">${n}</code><span style="color:var(--d-muted);flex:1">${d}</span><span style="color:var(--d-dim);font-size:11.5px">${place}</span>${act}</div>`;
    };
    const list = open ? `<div style="display:grid;gap:4px;padding:2px 0 4px">${l.skills.map(skillLine).join("")}</div>` : "";
    return gitRow({ icon: BOOK, name: l.name, account: l.source, acts, chips: chip(`${l.skills.length} skills`) + where(l) + chip(l.ver === "folder" ? "folder" : l.ver, l.ver === "folder" ? "Read from a folder on build-box" : "Commit"), more: list });
  };
  const addForm = () => `<div class="dcred-form" style="padding:10px 0;border-bottom:1px solid var(--d-border);display:grid;gap:8px">
      <b style="font-weight:600;font-size:13px">Add a library</b>
      <div class="dfield" style="gap:8px"><div style="display:flex;gap:8px"><input style="flex:1;font-family:var(--mono);font-size:12.5px" placeholder="Git URL or folder on build-box" autocomplete="off" spellcheck="false" aria-label="Git URL or folder">
      <button class="dbtn pri" ${dsetGo("skills")} data-toast="firecrawl added: 4 skills">Add</button><button class="dbtn quiet" ${dsetGo("skills")}>Cancel</button></div></div>
      ${note(`Each folder with a ${cd("SKILL.md")} is one skill. The library name is the repository or folder name.`)}</div>`;
  const found = v => {
    if (v === "skills-scanning") return `${head("Found on your machines", 18)}<div class="dmuted" style="display:block">Looking in each agent's skills folder on 3 machines…</div>`;
    if (v !== "skills-found") return "";
    return `${head("Found on your machines", 18)}${note("Global skills that come from a Git repository, grouped by repository. Adding one moves its skills out of the global folder; install it globally to keep them there.")}
      ${FOUND.map(f => gitRow({ icon: BOOK, name: f.name, account: f.source, acts: `<button class="dbtn" ${sm} ${dsetGo("skills")} data-toast="${f.name} added as a library">Add as library</button><button class="dbtn quiet" ${sm} ${dsetGo("skills")}>Ignore</button>`, chips: chip(`${f.skills} skills`) + chip(f.where) })).join("")}`;
  };
  const skillsBody = v => {
    const actions = `<div style="display:flex;gap:8px;margin-top:8px"><button class="dbtn" style="font-size:11.5px;padding:3px 9px" ${dsetGo("skills-add")}>+ Add a library</button><button class="dbtn quiet" style="font-size:11.5px;padding:3px 9px" ${dsetGo("skills-scanning")}>Find existing libraries</button></div>`;
    if (v === "skills-none") return `${note("A library is a pack of skills from a Git repository or a folder. Install it in the projects that need it, or globally.")}${actions}`;
    return `${LIBS.map(l => libRow(l, v)).join("")}${v === "skills-add" ? addForm() : actions}${found(v)}`;
  };

  // The project's Skills… sheet: one row per library, tri-state when only some skills are picked.
  const projectSheet = () => {
    const box = (state, label) => `<input type="checkbox" class="dcheck"${state === true ? " checked" : ""}${state === "some" ? ' data-mixed="1"' : ""}${state === "global" ? " checked disabled" : ""} aria-label="${label}">`;
    const libLine = (l, state, sub, open) => `<label class="am-line" style="display:flex;gap:8px;align-items:center">${box(state, l.name)}<b>${l.name}</b><span style="color:var(--d-muted);font-size:12px">${sub}</span>${open === null ? "" : `<button class="dbtn quiet small" style="margin-left:auto" data-dact="dset:pskills:${open ? "1" : "open"}" data-dgo="dWinProject" aria-expanded="${open}">${open ? "Hide" : "Choose skills"}</button>`}</label>`;
    const open = S.d.pskills === "open", committed = S.d.pskillsGit === "commit";
    const [fc, three, writing] = LIBS;
    const picked = ["blender-modeling", "blender-render", "gltf-export"];
    const skillLines = three.skills.map(([n, d]) => `<label class="am-line" style="display:flex;gap:8px;align-items:center;padding-left:24px"><input type="checkbox" class="dcheck"${picked.includes(n) ? " checked" : ""} aria-label="${n}"><code style="font-size:11.5px">${n}</code><span style="color:var(--d-muted);font-size:12px">${d}</span></label>`).join("");
    return `<div class="am-head"><b>Skills in acme/widget</b><kbd>esc</kbd></div>
      ${libLine(fc, true, `${fc.skills.length} skills`, null)}
      ${libLine(three, "some", `${picked.length} of ${three.skills.length} skills`, open)}${open ? skillLines : ""}
      ${libLine(writing, "global", "Installed globally", null)}
      <label class="am-line" style="display:flex;gap:8px;align-items:center;margin-top:8px"><input type="checkbox" class="dcheck"${committed ? "" : " checked"} data-dact="dset:pskillsGit:${committed ? "" : "commit"}" data-dgo="dWinProject">Keep out of Git<span style="color:var(--d-muted);font-size:12px">${committed ? "Installed skills show as changes you can commit" : `Listed in ${cd(".git/info/exclude")}`}</span></label>
      <div class="am-sub">Installed in ${cd(".claude/skills")} and ${cd(".agents/skills")} on every machine with this project. Skills you wrote there stay.</div>
      <div class="am-line ip-acts"><button class="dbtn quiet" style="margin-right:auto" data-dact="dset:set:skills" data-dgo="dWinSettings">Manage libraries…</button><button class="dbtn quiet" data-dact="dset:pskills:" data-dgo="dWinProject">Cancel</button><button class="dbtn pri" data-dact="dset:pskills:" data-dgo="dWinProject" data-toast="Skills saved for acme/widget on 3 machines">Save</button></div>`;
  };
  // Checkboxes cannot be indeterminate from HTML alone.
  new MutationObserver(() => document.querySelectorAll('input[data-mixed]').forEach(i => { i.indeterminate = true; })).observe(document.body, { childList: true, subtree: true });
  return { skillsBody, projectSheet };
})();
