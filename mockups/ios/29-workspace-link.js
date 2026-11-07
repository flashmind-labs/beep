// Workspace pairing is independent of the server account.
(() => {
  const settings = P.iSettings;
  P.iSettings = () => settings().replace(/<div class="body"([^>]*)>/, `<div class="body"$1>
    <div class="label">Workspace</div><p class="muted">Projects and node sessions connect directly. Your desktop can be offline.</p>
    <button class="primary" data-igo="iWorkspaceLink">Link workspace</button>`);
  P.iWorkspaceLink = () => iBack('Link workspace', 'iSettings') + `<div class="body">
    <p>On desktop, open Machines, Add a machine, then choose Phone. Copy its ticket here.</p>
    <label for="mesh-ticket">Phone pairing ticket</label><textarea class="field" id="mesh-ticket" placeholder="beepjoin2..."></textarea>
    <p class="muted">This phone gets project controls, node sessions and shared credentials.</p>
    <button class="primary" data-work-go="workProjects" data-itoast="Workspace linked">Link workspace</button></div>`;
})();
(() => {
  const project = P.workProject;
  if (project) P.workProject = () => project() + `<div class="body"><div class="label">Manage project</div>
    <button class="text-btn" data-itoast="Project pinned">Pin to top</button><button class="text-btn" data-itoast="Sync paused">Pause sync</button>
    <button class="text-btn" data-itoast="Removed from workspace, files kept">Remove from workspace</button></div>`;
})();

IOS_SCREENS.push({ group: "Workspace pairing", items: [["iWorkspaceLink", "Link workspace", "LinkedDevicesView"]] });

// With server features disabled, the app opens its workspace.
(() => {
  const views = P;
  const projects = views.workProjects;
  views.workProjects = () => projects().replace('data-work-go="iChats"', 'data-work-go="iWorkspaceLink"')
    .replace('<span class="title"', '<button class="text-btn" data-igo="iWorkspaceLink">Devices</button><span class="title"');
})();
