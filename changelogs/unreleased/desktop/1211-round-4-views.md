### Fixed
- Home, the project lists, workflow runs, usage and agent accounts follow the latest update at once instead of asking again after each change you make.
- Removing a project or a machine no longer brings back its old sessions, places or git state when you open the page again.

### Performance
- Importing a project or a repository from open items no longer reads all of Home a second time.
- The app keeps one copy of each project's sessions, places and git state, and lets go of the projects you used least.
