# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `vas --wide`: two more columns, `MACHINE` and `HOST`, the ids the API gives (`Instance::machine_id`,
  `Instance::host_id`), for a rentals log and for the `--exclude-*` flags.
- `vas offers --exclude-machine ID` (repeatable, accumulated with the config file's), beside
  `--exclude-host`: sent as `machine_id notin [...]` (checked live, 2026-10-17: the server applies it,
  and still answers 64 rows), re-checked on each row, named in the footer (`not machines ...`).
  `OfferQuery::exclude_machines`.
- The footer of `vas offers` names the time of the search (`searched 15:42:07 UTC`): offers vanish
  within minutes, so a list read later says how old it is. `render::clock`.

### Changed

- The money line's burn counts the boxes the API reads as `running` AND those it was asked to run
  (`intended_status`) that read `offline`, `loading` or anything else, and names them by state
  (`burn $1.940/h over 3 running + 1 offline (meant to run, billing unknown)`): one such box,
  during Me36, was left out of the burn although it may still bill, which overstated the runway.
  `Instance::is_meant_to_run`. A box being stopped (running, meant to stop) is still counted as
  running; a stopped one is not counted.
- BREAKING for library users (0.x: a minor release): `render::table` takes a `wide` flag (after
  `with_ssh`), and `render::offers_line` the time of the search (`now`, Unix seconds, last).
  `vas`'s own output gains the `searched ...` clause on the `vas offers` footer.

## [0.1.0] - 2026-10-17

First release.

### Added

- `vas`: one table per call over the account's vast.ai instances (status, `$/h`, age, estimated
  cost so far, `ssh` endpoint) and a money line (credit, burn rate of the running instances, hours
  left at that rate).
- `--ssh`: one parallel probe per running box over the system `ssh` (`nvidia-smi` utilisation and
  memory), one attempt each, never retried; `--log` adds the newest match's last two path
  components and its last non-blank line (`s0/train.log: ...`, so a glob over sequential runs says
  which run wrote it), or `(empty)`, `(not a file)`, `(no file matches)`, `?` (`probe::LogReading`);
  the box sends at most 4096 bytes of it, and both pipes are drained while `ssh` runs; `-i` picks
  the private key.
- `--watch SECONDS` (at least 30) and `--key-file FILE`.
- When no key is found and a SEARCHED key file is missing while its `.txt` twin exists (Windows
  Notepad's `vast_api_key.txt`), the error names the twin and the rename to make
  (`key::txt_hint`); `--key-file` is not concerned.
- `vas offers`: the offers for rent that pass `--gpu`, `--num-gpus`, `--disk`, `--cuda`,
  `--reliability`, `--inet-up`/`--inet-down`, `--max-price`, `--min-days`, `--verified`
  (`--no-verified`), `--exclude-country` and `--exclude-host` (repeatable), shown `--top N`,
  `--sort price|reliability|up`. Keyless: `GET /api/v0/bundles/?q=<JSON>` with no
  `Authorization`. Every constraint is sent AND re-checked on each row (a contradicting field drops
  the row, a missing one keeps it with a dash); `--disk` (else 5 GB) is priced into `$/h`. The
  footer names the constraints and the storage, counts the rows dropped, and says when 64 rows came
  back (vast.ai's cap, answered with `truncated: false`: the list may be cut).
- The `vas offers` config file, a file of the same flags (`$XDG_CONFIG_HOME/vast-skopos/offers`,
  `~/.config/vast-skopos/offers`, or `--config FILE`; `--no-config`), read before the command
  line: single values are replaced, lists accumulate. A missing `--config` file, an unreadable
  file or a malformed line stops the run (`SkoposError::ConfigFile`, `SkoposError::Config`). With
  no constraint at all, the identity (every offer rentable now), said on stderr.
- The library half: `key` (the official CLI's key search order, a redacted key type), `api`
  (read-only client, paginated `/api/v1/instances/`, `/api/v0/users/current/`; the keyless
  `SearchClient`), `instance` (lenient field-by-field readers), `offer` (`Offer`, `OfferQuery`,
  `OfferSort`, `OfferPage`, `select`), `config`, `probe`, `render` (`offers_table`,
  `offers_line`).
- Tests against a local mock server (pagination, the bearer header, statuses, no key in errors;
  the offer search's decoded query, no `Authorization`, its hints), the `vas offers` binary against
  the mock with no key anywhere, rendering fixtures (five real offer rows), doctests; CI on Linux,
  Windows and macOS at MSRV 1.88 and stable; `cargo-deny`.

### Changed

- The lenient readers keep finite numbers only (a numeric string can spell `NaN` or `inf`) and
  drop control and invisible characters (bidirectional overrides and isolates, zero-width
  characters, the line separators, the byte-order mark) from text, which comes from other
  people's hosts; a host-written cell keeps at most 40 characters.
- Output goes through one write that a closed pipe ends quietly (`vas offers | head -3`), where
  `print!` panicked.
- A redirect status says that `vas` follows none.
