# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Documentation

- The FAQ explains why `vas --ssh --log /root/...` typed in Git Bash reads `(no file matches)` (Git Bash rewrites
  the path into a Windows one before `vas` sees it), and the three ways around it: a glob, `//root/...`, or
  `MSYS_NO_PATHCONV=1`. The waiting tutorial points to it, since a 0.3.0 `--until` wait on such a path never
  ends (seen 2026-10-26).

## [0.3.0] - 2026-10-26

### Added

- vast.ai's status message for a box that is not running, on a line under the table before the money
  line: `57340012 loading: 46b4edaa8d47: Pull complete` (cut at 120 characters). The difference between a
  box that is progressing and one that is stuck (dogfooding 2026-10-23, note 5). `--compact` carries every
  box's message as `status_msg` (`null` when none). `render::status_lines`.
- A failed probe says whether `ssh` reached the box, and the box's age: `UNREACHABLE 4m: ssh: connect to host
  ... Connection refused` (refused, timed out, no route, a name that does not resolve, closed or reset before
  the session, nothing back within 30 s) or `SSH FAILED 4m: ...` (a refused key, a changed host key, no GPU
  reading, or a probe the box began to answer and did not finish within 30 s); the age is what the choice between waiting and destroying rests on (note 1a). `--compact`'s probe
  error object gains `error_kind` (`unreachable` or `failed`). `SkoposError::is_unreachable`,
  `render::failure_cell`.

- `vas offers --exclude-deverified` (and `--no-exclude-deverified`, to cancel a config file's): leaves out
  the machines vast.ai has deverified and keeps the unverified ones, which `--verified` hides too (note 9:
  an unverified box ran at full power and as fast as the fastest measured, a verified one was capped at
  475 W, a deverified one never started). The search ignores a filter on `verification` (probed
  2026-10-25), so it is sent as `vericode <= 1`, and not beside `verified`, which already leaves the
  deverified out (given both, the server applied only one). Footer: `not deverified`.
  `OfferQuery::exclude_deverified`.
- `vas offers --host ID` and `--machine ID` (repeatable, accumulated with the config file's): only these
  hosts or machines, to see what a host rented from before offers now, or whether the machine you had is
  for rent again (note 7: answering "is this the box we had?" took the raw API). Sent as `host_id` and
  `machine_id` `in [...]`, in the same object as the `--exclude-*` lists' `notin` (probed 2026-10-25: the
  server applies both), re-checked on each row, named in the footer (`hosts ...`, `machines ...`).
  `OfferQuery::only_hosts`, `OfferQuery::only_machines`.
- `vas offers --wide`: what tells one machine from another, `MACHINE` after `HOST`, then `CPU`, `DRIVER`,
  `DLPERF`, `DLPERF/$` (computed against the `$/H` shown, storage included), `PCIE GB/s` and `LIMIT W` (the
  GPU power limit the listing gives; on 2026-10-25 a machine of host 406325 listed 500 W, the cap `vas --ssh`
  read on two other machines of that host), with `WHERE` still last (and `SEEN` before it in a watch).
  `vas --wide` gains `CPU`, `DRIVER`, `DLPERF` and `PCIE GB/s` after `MACHINE` and `HOST`, so a rented box and
  an offer can be set side by side (note 7). DLPerf is a plain number: it did not show one host's 500 W cap on
  two machines (note 8's counterexample). `--compact` carries `cpu_name`, `driver_version`, `dlperf` and `pcie_bw` on
  every box. `Offer::cpu_name`, `driver_version`, `dlperf`, `pcie_bw`, `gpu_max_power`,
  `Offer::dlperf_per_dollar`; `Instance::cpu_name`, `driver_version`, `dlperf`, `pcie_bw`;
  `render::OfferColumns`.
- `vas --ssh --log PATH --grep PATTERN`: the last line MATCHING an extended regular expression (`grep -E`)
  instead of the last line, so a line printed once and buried by the next ones is caught (note 2: the
  first training loss of a stage, followed within seconds by its evaluation lines). Searched in the file's
  last 64 MiB within 10 s (one `timeout` over the whole search; `?` past it, and for a file the search cannot
  read), each carriage-return segment a line, empty lines left out (a CRLF log would leave one after every
  line), each line's first 4096 bytes (a long line is shown from its head), NUL bytes read as text
  (`grep -a`); `(no line matches)`, `(grep refused the pattern)` and `(a blank line)` (a line of spaces only)
  when that is what happened (`--compact`: `no_matching_line`, `pattern_refused`, or a `line` that is
  empty). `tail`'s and `grep`'s complaints are not on stderr, where they would stand in for `ssh`'s reason. The pattern is single-quoted for the box's shell, never expanded (a rule in
  CONVENTIONS.md), and `grep`'s status is the answer's last line, so no log line can stand in for it.
  `probe::LogQuery`, `LogReading::NoMatchingLine`, `LogReading::PatternRefused`.
- `vas --ssh --log PATH --watch N --until PATTERN`: wait for a line matching `PATTERN` (as `--grep` reads it,
  which it replaces) on any box, then exit 0 with the line on stderr; exit 2 at once when a box's `grep` refuses the pattern
  and no other box was read; exit 3 when no box could be read for 2 looks in a row (every probe failed or
  gave no readable log, or none could be made); exit 1 when the first look fails or stdout is closed before
  a match. A box meant to run but not running keeps the wait going for 10 looks at most since a box was
  last read (one whose host went offline must not hold it forever), and a look whose instances
  request failed does not count (note 2: the rule of the hand-written `ssh` loops it replaces, in one place).
  `until::verdict`, `until::Verdict`, `until::Wait`, `until::End`, `until::UNREAD_LOOKS`,
  `until::STARTING_LOOKS`.

### Changed

- BREAKING for library users: `probe::remote_command`, `probe::ssh_command`, `probe::ssh_probe` and
  `probe::probe_all` take an `Option<&LogQuery>` (`LogQuery::new(path)`, `.grep(pattern)`) instead of the
  `--log` pattern as a `&str`.
- BREAKING for library users: `render::offers_table` and `render::offers_table_marked` take an
  `OfferColumns` (after `top`): the plain table is `OfferColumns::Plain`.
- `LogReading` gains `NoMatchingLine` and `PatternRefused`, and `Instance` and `Offer` gain fields: all are
  `#[non_exhaustive]`, so a `match` already had a wildcard and no struct was built field by field.
- The `LAST LOG / SSH ERROR` cell of a failed probe no longer repeats `failed to probe HOST:PORT over ssh`:
  the `SSH` column already names the target. `--compact`'s `error` keeps the whole text.

### Documentation

- A second tutorial, [Waiting for a line](docs/tutorials/waiting-for-a-line.md), from a live wait on two
  rented RTX 5090s (2026-10-25). The FAQ gains which machine is which, what DLPerf and `LIMIT W` can and
  cannot say, verified against unverified against deverified, a box that stays `loading`, `UNREACHABLE` and
  `SSH FAILED`, wait or destroy, `--grep`, and waiting with `--until`; the first tutorial uses
  `--exclude-deverified`, the status line and the failure labels.
- After the 2026-10-26 rental ([its report](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-26.md)):
  - the FAQ explains a board limited in its firmware: `450W` reads as uncapped while idle, and shows as
    `+power_cap` and a lower clock under load;
  - it gives what 575, 500 and 450 W cost per step on one job: +8 % and +15 %, not shown in the price;
  - it records the idle card reading 100 % again, DLPerf seeing some limits and missing others, and `LIMIT W`
    matching host 406325's cap on a second day;
  - it says that `status_msg` stays once a box runs, that a direct route may be closed, that an `ssh` which says
    nothing reads `SSH FAILED`, and that `--until` ends on the first box that matches (and how to wait for each);
  - the two tutorials say the same.

### Security

- `Cargo.lock`: `rustls-webpki` 0.103.16 and `webpki-roots` 1.0.11 (RUSTSEC-2026-0382, RUSTSEC-2026-0381:
  the distrusted emSign roots), what `cargo install vast-skopos --locked` builds with.

## [0.2.0] - 2026-10-19

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
- `vas offers --watch SECONDS` (at least 20) searches again until Ctrl+C: each look is the whole table
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
  `render::offers_footer`). `VAS_WATCH_FLOOR_MS` lowers the floor of both watches, for tests only.
- `vas --credit-log FILE`: appends one CSV row per look (`unix_time,credit,burn_per_hour,running,unsure`,
  a header when the file is new or empty) so the credit becomes a curve and a box that bills while it reads
  `offline` shows as a credit that falls slower than the burn. Append-only, only when asked, never the key;
  a file that cannot be written is said once and the look goes on. The first file this crate writes:
  `creditlog::CreditLog` and a new rule in CONVENTIONS.md ("When Writing a File").
- The power cap, under `vas --ssh`: a `POWER` column after `OTHER` (`400/575W+power_cap`: the limit set on
  the card, the default when it is higher, and what holds the clocks down: `power_cap`, `thermal`, `slowdown`,
  `power_brake`) and, in `--compact`, per GPU `power_w`, `power_limit_w`, `power_default_w`, `sm_mhz`,
  `sm_max_mhz` and `throttle`. Seen on two rented RTX 5090s: 400 W against the 575 W default, SM clock held
  at ~1,700 of 3,090 MHz by the software power cap, with 100 % utilisation. (`+power_cap` alone is normal
  under full load: a default-limit RTX 4060 Ti read `165W+power_cap` at ~2,565 of 3,105 MHz under a matmul
  loop, seen live on 2026-10-19. A limit below the default with a clock far under the maximum is what marks
  a limit set on the box.) A second `nvidia-smi` query, so
  an old driver costs only these cells. `probe::GpuReading` gains the six fields (it is `#[non_exhaustive]`),
  `GpuReading::throttle_words`.
- Someone else on the GPU, under `vas --ssh`: an `OTHER` column after `VRAM` with the memory held by processes
  outside the container (`15.7G`: the device's memory in use minus what the box's compute processes hold; `-`
  below 1 GiB or when the box did not say), a line in a watch when it appears or goes (`! 56533728 gpu 0: memory
  held outside the container 0.0 -> 15.7 GiB`), `other_mem_mib` per GPU and `other_mib_was`/`other_mib_now`
  on the event in `--compact`. Seen on a rented RTX 5090: 24,809 MiB in use, 8,720 its own, and about half the
  GPU's time taken by someone it could not see. A third `nvidia-smi` query (the GPUs' bus ids, then the compute
  processes' memory per bus id, ended by a success line), so an old driver costs only this cell.
  `GpuReading::other_mem_mib`, `GpuReading::other_user_mib`, `probe::OTHER_MIB_MIN`.
- The clock, under `vas --ssh`: a `CLOCK` column after `POWER` (`1702/3090` MHz: now over the highest the card
  reaches) and, under `--watch`, a `CLOCK TREND` column after `TREND` with the last six samples of each box
  (the mean SM clock of its GPUs, oldest first, `3000 2900 1700 1700 1700 1700`), and `probe.clock_trend` in
  `--compact` (`null` in a one-shot). `Trend::clock_samples`, `Trend::clock_text`.
- A watch tells when a cap arrives or leaves: under `vas --ssh --watch N`, a line under the money when a GPU's
  power limit or throttle reasons change between two looks (`! 56533728 gpu 0: power limit 575 -> 400 W,
  power_cap began`), and in `--compact` an `event` object before the money one (`id`, `gpu`, `limit_was`,
  `limit_now`, `began`, `ended`). A box's first look is the baseline; a reading the box did not give changes
  nothing. `trend::GpuChange` (power limit, throttle reasons, other users), `Trend::changes` (what the last
  `record` found).
- Every `vas offers` search ends with a line saying that vast.ai's answers vary from one search to the
  next (a watch says it under its first look, `--changes` does not): `render::offers_variance_note`.
- The GPU trend: under `vas --ssh --watch N` the table gains a `TREND` column after `CLOCK`, the last six
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

- Documentation for people, not only for the code: [`docs/FAQ.md`](docs/FAQ.md) (the questions that came up on
  real rentals), [`docs/tutorials/from-filters-to-a-running-box.md`](docs/tutorials/from-filters-to-a-running-box.md)
  (one rental, step by step, with its safety warnings) and [`docs/roadmap-v0.2.0.md`](docs/roadmap-v0.2.0.md) (what
  0.2.0 contains and does not, the known limits, the candidates for later). Each claim in the first two was checked
  against the code and the tests, and the tutorial was read as a newcomer would.

### Changed

- The `--watch` floor, 30 s for both watches until now (a guess made before vast.ai's limits were measured), is
  10 s for `vas --watch` and 20 s for `vas offers --watch`: a `--watch` under it is still raised to it, and
  said on stderr. vast.ai's own limits are read and obeyed whatever the floor says (the instances list allows
  one call per window of about 3 s), so the floor is a courtesy to the service. 10 s tells a power cap, a
  clock drop or another user of the GPU sooner; 20 s keeps a table from scrolling past unread (a terminal
  that redraws in place, and 5 s for the offers, wait for a TUI).

- BREAKING for library users: a 429 is no longer `SkoposError::Status { status: 429, .. }` but
  `SkoposError::RateLimited { .. }` (the enum is `#[non_exhaustive]`, so a `match` already had a wildcard).
  The new variant also shifts the position (discriminant) of the variants after it; nothing should depend on
  that, the enum having no `repr`.
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
- BREAKING for library users (0.x: a minor release), settled together just after 0.1.0:
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
- A `--watch` shorter than its floor is raised to it, and `vas` says so on stderr (the floor is 10 s for
  `vas --watch` and 20 s for `vas offers --watch`, see Changed).
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

(Found by a second, five-part audit of the whole crate, 2026-10-19.)

- A garbled or hostile box can no longer panic a watch: the mean of the GPUs' utilisation or clock was summed
  in `u32`, so two saturated readings (`4294967295`, or `1e30` W read as `u32::MAX`) overflowed it. It sums in
  `u64` now.
- Text from a box no longer carries characters that hide or reorder what is shown. The probe's log lines, the
  log's file name and `ssh`'s error line were stripped of control characters only, where the API's text lost
  the invisible ones too, so a log line with a right-to-left override could reorder a row. All of it now goes
  through `is_hidden`, which also learned the soft hyphen, the combining grapheme joiner, the deprecated format
  characters (U+206A-206F), variation selectors, interlinear annotations and the tag characters (U+E0000-E007F)
  that can spell a hidden message.
- `OTHER` no longer cries wolf: a GPU with NO process listed is unknown (`-`), not "all of its memory is
  someone else's". A run that has just restarted, or a box whose container cannot see its own processes, lists
  none while the memory is still its own, and a watch would have raised a false "memory held outside the
  container" line.
- The instances list no longer spends a request that its budget will refuse: before a second page it checks
  the budget, and `Client::instances_with(between_pages)` lets the caller wait the window out (the library
  still never sleeps). `Client::instances()` stops with `RateLimited` and the wait instead. An account with
  more than 25 instances needs a second page, and one call per window allows no second page at once.
- A `POWER` cell no longer hides what holds a card down when its limit is unknown: `-+thermal`, as `--compact`
  says in `throttle`.
- A throttle mask is read with `0x` or `0X` and never with a doubled prefix.
- A key file is one word: a second line, or a space or control character in the key, is refused when it is
  read, with a message that does not show the key (it used to fail at the first request with a message about
  a header). A truncated UTF-16 key or config file (an odd number of bytes) is refused instead of read as a
  shorter, valid one. A relative `XDG_CONFIG_HOME` is ignored, as the XDG specification says: it made the files
  searched depend on the folder `vas` runs from.
- An integer written as `123.0` or `"123.0"` is an integer (an `id` written that way used to drop the row
  silently), within what an `f64` holds exactly.
- `VAST_URL` host detection for the proxy bypass: the authority ends at the first `/`, `?` or `#`
  (`http://evil.example?@127.0.0.1` is not this machine), and `LOCALHOST` is `localhost`.
- `--min-days` is capped at 36500 (a hundred years): `1e305` days overflowed to infinity and was sent to the
  server as `null`.
- `RateLimit::from_headers` is `#[must_use]`.
- A driver that lacks one name of a field no longer costs the power and the clock: the clock event reasons
  are their own query, asked as `clocks_event_reasons.active` and, failing that, as the older
  `clocks_throttle_reasons.active` (NVIDIA deprecated it), where a single query naming the older field lost
  the power, the clock and the reasons together when a driver dropped it. `nvidia-smi` prints an unknown
  field's error on stdout and exits 2; the parsers skip that line. A fake `nvidia-smi` in the Linux tests
  answers like a driver that knows the new name, the old one, or neither.
- The log's last line is read from the last 4096 BYTES of the file, then the last 20 lines of those: a
  progress bar written for hours with no newline made `tail -n` scan the whole file under the box's 5 s
  `timeout`, which printed nothing and read as an empty file.
- Each probe has an `ssh` session of its own (`ControlMaster=no`, `ControlPath=none`): a `ControlPersist`
  master in the user's `~/.ssh/config` kept the pipes open and cost up to two seconds a probe.
- A `Retry-After` above a day is not taken as a wait (`1e300` parses, and printed as a 300-digit number of
  seconds): the budget headers speak instead. A row that says `ssh_port` 0 has no endpoint to probe. An
  explicit `--key-file` that is blank says so (`(that file is blank)`) instead of listing itself among the
  places searched.
- The README and the docs say what is true of: trust on first use for host keys, the throttle bits that come
  and go (each flip is said), a GPU the main query cannot read whole being left out (MIG slices), a machine
  listed under two ids at different prices, and `VAS_WATCH_FLOOR_MS` (for tests; it removes the courtesy
  floor and nothing else).
- The README says what is true: the probe columns' order (`GPU% (ssh)`, `VRAM`, `OTHER`, `POWER`, `CLOCK`, then
  `TREND` and `CLOCK TREND` in a watch), a "Try it" sample with those columns, the `--compact` example with
  every GPU key and the `event` kind, `kind` as `box`, `event` or `money`, and that the lookout prints no
  `since the last successful look` note. The module list of the crate docs names `error`, and the docs of
  `TableOptions` name every probe column. A test now compares the README's sample header, its probe-column
  order and its `--compact` example with what the code prints, so they cannot drift again unseen.
- `vas` waits out a short budget between two pages of the instances list (said on stderr:
  `waiting 1 s before the next page of instances`), where it used to fail the look with a 429 on the second
  page; a longer one ends the look like any other rate limit.
- A watch no longer sleeps for an absurd request budget. A reset in milliseconds (`1.79e12`) made it print
  "renews in 1790000000000 s" and sleep effectively forever (or panic above about `1.8e19`); it now waits at
  most 900 s and says "more than a watch waits".
- The note under an offers table counts a moved price as below the table when its ROW is (that is where
  `(was ..)` is shown), and a new machine when no row of it is (every row of it is marked `+`); the two
  rules used to disagree for a machine shown under another offer id.
- A label, a status and an `ssh` host from the API are capped like the other cells of remote text (the port
  after a host is never cut), so a 300-character one no longer widens every row.
- The 64-row cap note reads the cap from `OFFER_CAP`; `mib_to_gib` is one function, not two; the three
  per-GPU cells share `per_gpu`.
- `--help` is right: `--ssh` names what the probe reads (utilisation, memory, power limit, clock, throttle
  reasons, memory held by anyone else), `--log` names its `?` state, `vas offers` no longer says "cheapest
  first" as if `--sort` did not exist, and a test ties the `--watch` helps to the floors and the learning
  looks the code uses.

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
