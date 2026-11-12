### Fixed

- Desktop: on a flaky network the app no longer climbs to several cores of CPU and gigabytes of memory; the networking library kept retrying the same connection paths in a queue that never stopped growing.
