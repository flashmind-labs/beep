### Added

- Node: The `model_3d` generate action takes `polycount`, the most faces the model may have, for Meshy and Tripo. A lower count gives a smaller file that loads faster. Credits charged stay the same. Other provider options pass through to Meshy or Tripo.

### Fixed

- Node: The media tools (image, video, speech, model_3d, decision) now send every argument they accept to the provider. An unknown argument, a wrong type, or an argument that belongs to another action is refused with a message naming it. Before, asking Meshy for a low-poly model returned a full-detail one.
