### Changed

- Node: `beep-node mesh join` with a fresh ticket brings a removed node
  back into the mesh.

### Fixed

- Node: after a refused join on a removed node, running `beep-node mesh
  join` again no longer fails with "this directory already belongs to a
  mesh".
