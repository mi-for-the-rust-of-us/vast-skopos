# Roadmap: what 0.4.0 is, and what may come next

**Not affiliated with vast.ai.** This page says what `vas` 0.4.0 contains, what it deliberately does not,
and which ideas are on the table for later. The later part is a list of **candidates, not promises**:
each one says why it is wanted and what is still open. The authority for what shipped is the
[CHANGELOG](../CHANGELOG.md). The roadmaps of [0.3.0](roadmap-v0.3.0.md) and [0.2.0](roadmap-v0.2.0.md) are
kept as they were written, and [the plan for 0.5.0](roadmap-v0.5.0.md) says what comes next.

## What 0.4.0 contains

0.4.0 builds the candidates the 0.3.0 roadmap listed after the rentals of 2026-10-23 to 10-26. A test rental
of two RTX 3080s on 2026-10-28 ([its report](https://github.com/mi-for-the-rust-of-us/vast-skopos/blob/main/docs/dogfooding/2026-10-28.md))
checked it on real boxes and found one bug, fixed before the release.

**Reaching a box**
- **`DIRECT`, under `--wide`:** the box's own `ssh` address, past vast.ai's proxy (its public IP and the port
  mapped to 22).
- **The direct route, tried once when the proxy never reached the box.** One attempt per route, never a loop.
  Readings that came that way say `(direct)`; a box neither route reached says both reasons.

**Reading the card**
- **`450W<575`:** a board whose maximum is below its model's reference power. An idle look at such a board
  used to read like an uncapped card (2026-10-26). The table holds five models: RTX 5090, 5080, 4090, 4080
  and 3090.
- **`100%?`:** a GPU that reads busy with no memory held and no process, the stuck counter of an idle card
  (seen three times), with a line under the table.

**Waiting for a run**
- **`--instance ID`** (repeatable): the rows, the probes and `--until` follow the boxes named. The money line
  stays the whole account's, and says how many boxes are shown.
- **`--until` no longer waits forever on a path that matches no file.** It warns once, after 2 looks, and
  gives up with exit 3 after `--log-wait` seconds (900, 15 minutes, by default).
- **A `--log` path typed in Git Bash** (`/root/...`, which Git Bash rewrites as `C:/Program Files/Git/root/...`)
  is read as typed again, and said.
- **vast.ai's text keeps its line breaks as spaces:** a status message no longer reads `Verifying
  Checksumc455daa85bb6:`.

**For library users**
- Ten helpers that were public but hidden are now documented and promised: `probe::strip`, `clean`,
  `parse_log`, `ssh_command`; `render::age`, `short_path`, `log_cell`, `failure_cell`; `offer::gpu_model`,
  `country_code`.
- `GpuReading::new` and its setters build a reading by hand, to drive a `Trend` or a table without a box.
- Nothing breaks: `cargo semver-checks` against 0.3.0 passes all 196 checks.

**Robustness**
- Two independent reviews before the release: 11 findings, then 3 and 8 nits. Among them, a test that
  reached a real `ssh` proxy, and a wait that gave up too early. All were fixed or answered.
- Each new test was shown to fail when its rule was broken.
- CI on Linux, macOS and Windows (MSRV 1.88 and stable), and the test rental above.

## What 0.4.0 deliberately does not do

- **It does not rent, start, stop or destroy anything**, and will not: `vas` issues only `GET` requests.
- **It does not mark an offer as "likely capped".** `LIMIT W` has matched the card's limit on two hosts now
  (406325 and 102063). That is two hosts, not yet a rule.
- **It does not retry.** One attempt per route per look; `--until` waits by looking again.

## Known limits

- **Not yet seen live:**
  - the direct-route fallback, and `UNREACHABLE` on a box that really refuses (every `ssh` answered on
    2026-10-28);
  - `100%?` (the idle RTX 3080s read 0 %);
  - the `<575` mark (the RTX 3080 has two reference powers, so it is not in the table);
  - `--log-wait` at its full length.

  All are tested end to end with a fake `ssh`; none can be made to happen on a rental.
- **On some `ssh` builds a refused port reads `SSH FAILED …: no GPU reading (ssh exit 255)`**, not
  `UNREACHABLE`: GitHub's Windows runner's `ssh` exits 255 and says nothing, and `vas` does not guess.
- **The direct route can double a probe's worst case,** to about 60 s for a box neither route reaches.
- **Some cards report `power_cap` while idle** (an RTX 3080 at 0 % and 210 MHz, 2026-10-28). Read `+power_cap`
  beside the limit and the clock, never alone.
- **A power-limit change cannot be tested from a box:** a container cannot change its card's limit (`nvidia-smi
  -pl` answers `Insufficient Permissions`).
- **The Git Bash repair reads `/bin/...` as `/usr/bin/...`,** as Git Bash's own mapping implies.
- And, from earlier versions:
  - `--grep` searches only the last 64 MiB of a file;
  - why the search flickers is not known;
  - `COST~` is an estimate, and the storage of stopped boxes is not shown.

## Candidates for later

Not a priority order, and none promised.

1. **Making `vas` easy to pick up,** for a person and for an AI assistant. This is [the plan for
   0.5.0](roadmap-v0.5.0.md): two levels of help, a skill for agents, a lighter README top, and the learning
   curve measured.
2. **`LIMIT W` as an early warning,** if it keeps matching the cards' caps on more hosts.
3. **A history of the boxes seen** (note 3 of 2026-10-23): instance, machine and host, first and last status.
   *Open:* it would be the second file `vas` writes, a change of rule with its own review.
4. **A count of the offers a filter hid** (note 6): `3 deverified offers hidden`. *Open:* the count needs the
   unfiltered answer, which a server-side filter does not give.
5. **Still open from 0.2.0:**
   - a text interface for the watch, and a quiet watch;
   - a measured upload (`vas check`);
   - the storage cost of stopped boxes;
   - a process's share of the GPU against the card's utilisation.

   See [the 0.2.0 roadmap](roadmap-v0.2.0.md) for what each needs.
6. **A question still unanswered:** note 4 of 2026-10-23, whether `vas offers --watch` helped the hunt and
   what was missing.

## How to influence it

The roadmap comes from real rentals. If something bit you on yours, [open an issue](https://github.com/mi-for-the-rust-of-us/vast-skopos/issues)
with the figures: the command, the output, and what you expected.
