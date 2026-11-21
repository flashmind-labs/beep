### Added

- Node: flsh's config now carries each model's own quantizations.
- Node: before pi starts, the OpenRouter routing settings are written into
  pi's models.json, and the user's other entries are kept. A models.json
  with comments is left alone, and pi does not start. The error says so.
