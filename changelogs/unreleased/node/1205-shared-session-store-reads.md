### Performance

- Node: session lists, project session pages, search and file previews no
  longer each read every coding agent's session files from disk. The node
  reads them once and shares the result, which cuts its disk work when many
  sessions are stored.
- Node: listing sessions no longer looks through every Claude Code project
  folder for each running session.
