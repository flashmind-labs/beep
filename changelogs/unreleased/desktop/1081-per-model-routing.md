### Added

- Desktop: on the Coding agents page, each model in flsh's per-model
  list can have its own quantizations (all, any precision, or a picked
  list) and its own auto-compact (same as all models, on, or off with a
  threshold). A model whose only OpenRouter provider serves another
  precision now works without loosening the list for every model.
- Desktop: pi's Coding agents page shows the OpenRouter routing settings:
  zero data retention, quantizations, provider sort, fallbacks, and
  per-model quantizations. flsh and pi share them.
