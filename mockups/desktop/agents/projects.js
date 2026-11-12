DESK_SCREENS.find(g => g.group === "Command center").items.push(
  ["dAgentNew", "Start a coding agent", "CommandCenterWindow, AgentNew"],
  ["dAgentSession", "Coding agent session", "CommandCenterWindow, AgentSession"],
  ["dAgents", "All coding agents", "CommandCenterWindow, AgentList"]);

// Every window view now shows the projects in the sidebar.
{
  for (const id of ["dWinSettings", "dAddMachine", "dHome"]) {
    const base = D[id];
    D[id] = (...a) => base(...a).replace(/<div class="cc-grp"><h3>Projects<\/h3>[\s\S]*?<\/div>/, () => {
      const cur = id === "dHome" ? "home" : "";
      const t = document.createElement("template");
      t.innerHTML = ccSidePj(cur);
      const g = [...t.content.querySelectorAll(".cc-grp")].find(x => x.querySelector("h3")?.textContent.startsWith("Projects"));
      return g ? g.outerHTML : "";
    });
  }
}
DESK_SCREENS.find(g => g.group === "Command center").items.push(
  ["dWinProjects", "Projects overview", "CommandCenterWindow, ProjectsView"],
  ["dWinProject", "Project", "CommandCenterWindow, ProjectView"]);

// Links to the session usage variants open them directly.
{
  const usagePreview = new URLSearchParams(location.search).get("usage");
  if (location.hash === "#dAgentSession" && ["flshusage", "flshunknown"].includes(usagePreview)) {
    S.screen = "dAgentSession";
    S.d.ags = usagePreview;
  }
}

// Reopened sessions show the latest turn. The scrolled-up variant stays put.
{
  const base = render;
  render = function () {
    base();
    if (S.screen !== "dAgentSession" || S.d.ags === "richjump") return;
    const body = document.querySelector("#desk #agbody");
    if (body) body.scrollTop = body.scrollHeight;
  };
}

// Worktree menu controls preserve the chosen checkout when following is paused.
document.addEventListener("click", event => {
  const button = event.target.closest("[data-wtfollow]");
  if (!button) return;
  S.d.wtfollow = button.getAttribute("aria-checked") === "true" ? "off" : "on";
  button.setAttribute("aria-checked", String(S.d.wtfollow !== "off"));
});
document.addEventListener("input", event => {
  if (!event.target.matches(".ag-wtsearch")) return;
  const query = event.target.value.trim().toLowerCase();
  for (const item of event.target.parentElement.querySelectorAll('[role="menuitem"]')) {
    item.hidden = !item.textContent.toLowerCase().includes(query);
  }
});
