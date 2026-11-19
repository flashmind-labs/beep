### Fixed

- beep-node: on networks that block DNS over TLS, lookups now use encrypted
  DNS over HTTPS instead of the network's own DNS. A name that public DNS
  says does not exist is no longer answered by the network's DNS, which some
  hotel and filtering networks rewrite.
