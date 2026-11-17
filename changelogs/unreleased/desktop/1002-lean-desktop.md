### Changed

- Desktop: default source builds provide a lean remote controller with plaintext replies. Build with the `full` Cargo feature or run `just run shared full` for local agents, terminals, sync, device and media hosting, rich text, and native integrations.
- Desktop: plugins load compiled JavaScript. TypeScript plugin authors must build before installing; shipped plugins continue to work.
