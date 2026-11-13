### Added

- Node: skill libraries. A node clones each library at the commit the desktop
  picked, then installs the chosen skills into the agents' skills folders and
  into project checkouts. Project skills kept out of Git are listed in
  .git/info/exclude. Unpicked skills are removed. A skill you edited, or one of
  the same name you made, is never overwritten or deleted, and skill sync
  leaves library skills out.
