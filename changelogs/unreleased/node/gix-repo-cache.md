### Performance

- Node: Git sync and the branch views keep each repository open between
  reads instead of loading its pack indexes again on every scan, which uses
  less CPU and battery.
