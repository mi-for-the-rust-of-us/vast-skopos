# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `vas`: one table per call over the account's vast.ai instances (status, `$/h`, age, estimated
  cost so far, `ssh` endpoint) and a money line (credit, burn rate of the running instances, hours
  left at that rate).
- `--ssh`: one parallel probe per running box over the system `ssh` (`nvidia-smi` utilisation and
  memory), one attempt each, never retried; `--log` adds the last line of the newest matching file;
  `-i` picks the private key.
- `--watch SECONDS` (at least 30) and `--key-file FILE`.
- The library half: `key` (the official CLI's key search order, a redacted key type), `api`
  (read-only client, paginated `/api/v1/instances/`, `/api/v0/users/current`), `instance`
  (lenient field-by-field readers), `probe`, `render`.
- Tests against a local mock server (pagination, the bearer header, statuses, no key in errors),
  rendering fixtures, doctests; CI on Linux, Windows and macOS at MSRV 1.88 and stable;
  `cargo-deny`.
