### Added

- Node: assistant chats get instructions for being the Beep assistant and
  project tools: `projects_hosts` lists Git hosts, `projects_create` creates
  a project (private repository unless asked), `projects_clone` clones a URL,
  `projects_found` lists the projects found on your machines and
  `projects_import` imports them.
- Node: the project tools need a desktop to be online. Agents that the
  assistant starts do not get them.
- Node: an assistant chat stays one after beep-node restarts or the chat
  moves to another machine, with its title, model and thinking level.
