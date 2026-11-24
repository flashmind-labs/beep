### Added

- Node: Lazily runs installed language servers (such as `rust-analyzer`,
  `vtsls`, `typescript-language-server`, `basedpyright`/`pyright`, `gopls`, and
  `clangd`) per workspace with an idle timeout to answer editor go-to-definition
  requests.
