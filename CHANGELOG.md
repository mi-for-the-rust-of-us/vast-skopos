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
  within minutes, so a list read later says how old it is.
- `vas --compact`: the same readings as JSON Lines, for scripts and monitors: one object per box
  (`kind: "box"`: status, `$/h`, age, cost, `ssh` endpoint, machine and host ids, and with `--ssh` the
  probe: GPUs, the log's state and last line, or the error), then one for the money (`kind: "money"`:
  credit, burn, running, the unsure boxes by state, hours left). The keys never change (`null` for
  what is not known), every object carries `at`, and `--watch` prints no separator line. Money is
  rounded to ten-thousandths of a dollar (the credit, the burn, the cost and the price per hour): vast.ai
  sends them with float noise (`0.4907407407407407`).
  Not with `--wide`. `compact::json_lines`, `runway::Runway` (the credit, the burn and the hours they
  last: the money line and the `money` object are both read from one).
- `vas offers --watch SECONDS` (at least 30) searches again until Ctrl+C: each look is the whole table
  with `+` on the new machines, `0.594 (was 0.575)` on a moved price, a `SEEN` column (before `WHERE`: in
  how many looks the machine appeared, `15/15` or `3/9`), and under the footer the counts and the
  machines that are gone. vast.ai's search answers a flickering SAMPLE of the offers (measured
  2026-10-18: fifteen identical searches saw 129 machines, 30 of them in every answer, and one machine
  under several offer ids), so the watch does not compare two answers: a machine is its machine id and GPU
  count (`market::OfferKey`), the first three looks only learn, `+` needs a machine absent from the
  previous three looks, and a machine is gone after five looks missing in a row
  (`market::MarketWatch`, with `LEARNING_LOOKS`, `ABSENT_FOR_NEW`, `ABSENT_FOR_GONE`). `--changes` prints
  only what moved, a line each (`+` new, `~` repriced, `-` gone, or `no change`; the learning looks say
  so). Offers are compared as the FULL lists the searches kept (an offer sinking below the table's last
  row is not gone), and a note says when vast.ai's 64-row cap may explain an entry or an exit.
  `OfferChanges` (and its `Repriced`), `render::offers_table_marked` (it takes the `MarketWatch`),
  `render::offers_changes_note`, `render::offers_changes_only` (the page first, like
  `render::offers_footer`). `VAS_WATCH_FLOOR_MS` lowers the 30 s floor of both watches, for tests only.
- `vas --credit-log FILE`: appends one CSV row per look (`unix_time,credit,burn_per_hour,running,unsure`,
  a header when the file is new or empty) so the credit becomes a curve and a box that bills while it reads
  `offline` shows as a credit that falls slower than the burn. Append-only, only when asked, never the key;
  a file that cannot be written is said once and the look goes on. The first file this crate writes:
  `creditlog::CreditLog` and a new rule in CONVENTIONS.md ("When Writing a File").
- Every `vas offers` search ends with a line saying that vast.ai's answers vary from one search to the
  next (a watch says it under its first look, `--changes` does not): `render::offers_variance_note`.

- The GPU trend: under `vas --ssh --watch N` the table gains a `TREND` column after `VRAM`, the last six
  samples of each box oldest first (`97 99 0 98 99 99`: the mean utilisation of its GPUs), and `--compact`
  carries them as `probe.util_trend` (`null` in a one-shot). One `nvidia-smi` sample is not a diagnosis: it has
  read 100 % with 0 MiB used while nothing held the card. In memory only; a failed probe adds nothing and keeps
  the history, a box that stops or leaves is forgotten. `trend::Trend`, `TableOptions::trend`; `json_lines`
  takes the trend after the probes.
- vast.ai's rate limit is read and obeyed. Every endpoint has a budget of calls per window, written in
  `X-RateLimit-Limit`, `-Remaining` and `-Reset` (seen 2026-10-18: 30 for the offer search, ONE for the
  instances list: a script that ran `vas` a dozen times in a minute met a 429 on the second call). The
  clients keep what each answer said (`api::RateLimit`, `Client::budget_wait_secs`,
  `SearchClient::budget_wait_secs`); a 429 is `SkoposError::RateLimited` with the wait (`Retry-After`, else
  the time to the reset): `vast.ai rate-limited /api/v1/instances/: try again in 7 s`. In `vas`, a look
  refused for a wait of 15 s or less waits it out once, says so on stderr and tries again; a longer wait is
  named and ends the run; a `--watch` leaves at least the reset between two looks. The library never
  sleeps or retries.

### Changed

- BREAKING for library users: a 429 is no longer `SkoposError::Status { status: 429, .. }` but
  `SkoposError::RateLimited { .. }` (the enum is `#[non_exhaustive]`, so a `match` already had a wildcard).
- `vas --ssh` asks the account for its credit WHILE the probes run, instead of before them: the
  request needs nothing from them, and they take seconds (`probe::probe_all`, which the binary used to
  hold, now takes the instances and returns one outcome per instance). `probe::ssh_command` (hidden
  from the docs) builds the `ssh` arguments apart from the spawning, so a test can check that a path
  with a space stays one argument and that the target follows `--`.
- Documentation: the README's install section says what is true now (`cargo install vast-skopos`,
  `version = "0.2"`), its three Rust examples are compiled by `cargo test --doc` (they cannot rot),
  the crate docs describe `vas offers`, `--compact` and `--watch`, `render::table` has a doctest, and the
  `probes` contract (one outcome per instance, in order) is stated where it is taken.
- Under the hood, no output changes: one table function for the offers (plain or marked), the cell
  helpers every table repeated, one ellipsis cut, one watch loop for `vas` and `vas offers`, one error
  constructor per kind in the API client and one for the `ssh` error, one reader for the key and the
  config file, and `admits` no longer rebuilds the country list for every row.
- The money line's burn counts the boxes the API reads as `running` AND those it was asked to run
  (`intended_status`) that read `offline`, `loading` or anything else, and names them by state
  (`burn $1.940/h over 3 running + 1 offline (meant to run, billing unknown)`): one such box,
  during Me36, was left out of the burn although it may still bill, which overstated the runway.
  `Instance::is_meant_to_run`. A box being stopped (running, meant to stop) is still counted as
  running; a stopped one is not counted.
- BREAKING for library users (0.x: a minor release), settled together while 0.1.0 is a day old:
  - `render::table(instances, probes, account, options, now)`: 0.1.0's `with_ssh` boolean (the order was
    `instances, probes, with_ssh, account, now`) became `TableOptions::new().ssh(..).wide(..)`, placed
    after `account`, and the probes a `&[ProbeOutcome]` (new alias of `Option<Result<Probe>>`).
  - `render::offers_line(page, kept, shown, ..)` became `render::offers_footer(page, kept, top, query,
    sort, now)`: the kept offers and `top`, not two counts that could be swapped; it names the search
    time (`now`, Unix seconds).
  - `SkoposError::Config` became `SkoposError::ConfigLine`, no longer easy to take for `ConfigFile`.
  - `OfferQuery::to_q` became `OfferQuery::to_search_json`.
  - Documented API narrowed with `#[doc(hidden)]`, which keeps the paths and promises nothing:
    `render::{age, short_path, log_cell}`, `probe::{strip, clean, parse_log}`,
    `offer::{gpu_model, country_code}`, `key::txt_hint` and the `config` module. `vas` uses `gpu_model`,
    `country_code` and `config`; the tests use `age`, `log_cell` and `txt_hint`; `short_path`, `strip`,
    `clean` and `parse_log` only their own doctests and the crate itself.

### Fixed

(Found by a review of the code since 0.1.0, before 0.2.0 shipped.)

- A config file's `--watch` or `--changes` is refused, naming the file and the line: a file of filters
  silently turned every `vas offers` into an endless loop, and `--changes` on the command line then failed
  with a confusing "`--watch` not provided".
- A `--watch` whose first look fails ends (exit 1) instead of repeating the error forever, for `vas offers`
  and for the lookout alike; once a look has succeeded, a failure is said and the watch goes on, and the next
  look reads `since the last successful look` (`--changes`: `(a search failed in between)`).
- The note under a watch's table counts the new and repriced offers that the table does not show:
  `1 new (1 below the table)`, where it used to count them with no mark on screen to match.
- `--changes` lists every change; only the note under a table caps its gone offers at ten.
- A `--watch` shorter than the 30 s floor is raised to it, and `vas` says so on stderr.
- An offer id answered twice in one search is one offer (the row that sorts first), not two rows marked and
  counted twice.
- A credit or burn that rounds to zero no longer prints as `$-0.00` or `-0.0`, and a burn too small to give a
  runway no longer prints `~inf h left`.
- A debt has no runway: a credit of zero or less prints no `~-5.0 h left` (`hours_left` is `null`).
- The footer of `vas offers` says `1 repeated id skipped` for an id the search answered twice, instead of
  blaming a constraint (`1 dropped here: a field contradicts a constraint`) with no constraint at all.
- The home is found the way the official CLI finds it: `USERPROFILE` first on Windows (`HOME` there is
  often an MSYS path no Windows call understands, so a key saved by `vastai` was not found from Git
  Bash), `HOME` elsewhere.
- A `VAST_URL` that names this machine is never sent through the proxy of the environment: the tests'
  mock server failed behind an HTTP proxy.

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
