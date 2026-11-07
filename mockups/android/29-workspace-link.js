// Workspace pairing is independent of the server account.
(() => {
  const devices = R.devices;
  R.devices = () => devices().replace('<div class="label" style="padding:4px">Devices on this account</div>', `
    <div class="label">Workspace</div>
    <p class="muted">Projects and node sessions connect directly. Your desktop can be offline.</p>
    <button class="primary" data-go="workspaceLink">Link workspace</button>
    <div class="hr"></div><div class="label" style="padding:4px">Devices on this account</div>`);
  R.workspaceLink = () => `<div class="bar"><button class="ib" data-go="devices" aria-label="Back">${I.back}</button><div class="title">Link workspace</div></div>
    <div class="body"><p>On desktop, open Machines, Add a machine, then choose Phone. Copy its ticket here.</p>
    <label for="mesh-ticket">Phone pairing ticket</label><textarea class="field" id="mesh-ticket" placeholder="beepjoin2..."></textarea>
    <p class="muted">This phone gets project controls, node sessions and shared credentials.</p>
    <button class="primary" data-work-go="workProjects" data-toast="Workspace linked">Link workspace</button></div>`;
})();
(() => {
  const project = R.workProject;
  if (project) R.workProject = () => project() + `<div class="body"><div class="label">Manage project</div>
    <button class="text-btn" data-toast="Project pinned">Pin to top</button><button class="text-btn" data-toast="Sync paused">Pause sync</button>
    <button class="text-btn" data-toast="Removed from workspace, files kept">Remove from workspace</button></div>`;
})();

SCREENS.push({ group: "Workspace pairing", items: [["workspaceLink", "Link workspace", "LinkedDevicesScreen"]] });

// With server features disabled, the app opens its workspace.
(() => {
  const views = R;
  const projects = views.workProjects;
  views.workProjects = () => projects().replace('data-work-go="drawer"', 'data-work-go="workspaceLink"')
    .replace('<span class="title"', '<button class="text-btn" data-go="workspaceLink">Devices</button><span class="title"');
})();

(() => {
  const projects = R.workProjects;
  R.workProjects = () => projects() + `<nav class="work-tabs" aria-label="Workspace navigation">
    <button data-work-go="workProjects">Projects</button><button data-work-go="workAgents">Agents</button>
    <button data-work-go="workMachines">Machines</button><button data-work-go="workTerminals">Terminals</button>
    <button data-go="workspaceLink">Devices</button></nav>`;
})();
